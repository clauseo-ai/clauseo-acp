//! With no sandbox wired (the Safe workspace parked for launch), the `sandbox.*` keys set, from
//! a policy or `settings.json`, and the one warning that names them: once per change, never
//! with a value or a path, and never with a sandbox wired.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use clauseo_sandbox::{Detection, Tier};
use clauseo_settings::policy::file;
use clauseo_settings::{Effective, Error, Locations, PolicyRead, PolicySource, SystemFiles};

use super::super::{ProtectionHost, observe, set_keys};
use crate::config::Sandbox;
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// A policy source whose `policy.json` text is `text`, read through the real parser.
struct Text(&'static str);

impl PolicySource for Text {
    fn name(&self) -> &'static str {
        "/etc/clauseo/policy.json"
    }

    fn read(&self) -> PolicyRead {
        match file::parse(Path::new(self.name()), self.0.as_bytes()) {
            Ok(values) => PolicyRead::Read(values),
            Err(error @ (Error::Parse { .. } | Error::DuplicateKey { .. })) => {
                PolicyRead::Unreadable(error)
            }
            Err(other) => panic!("{other}"),
        }
    }
}

/// The settings under `policy` (none when `None`) and the user's `settings` text, in a folder
/// named for `test`.
fn load(test: &str, policy: Option<&'static str>, settings: &str) -> Effective {
    let dir = TempDir::new(test);
    let locations = Locations::under_root(&dir.path().join("data"));
    let file = locations.settings_file();
    write::create_dir_all(file.parent().unwrap()).unwrap();
    write::write_file(file, settings.as_bytes()).unwrap();
    let text;
    let sources: Vec<&dyn PolicySource> = match policy {
        Some(policy) => {
            text = Text(policy);
            vec![&text]
        }
        None => Vec::new(),
    };
    Effective::load(&locations, &SystemFiles, &sources)
}

#[test]
fn a_key_counts_as_set_from_a_policy_or_settings_json_and_only_then() {
    let required = ["sandbox.required"];
    let auto = ["sandbox.allow_unprotected_auto"];
    let both = ["sandbox.required", "sandbox.allow_unprotected_auto"];
    let rows: [(Option<&'static str>, &str, &[&str]); 7] = [
        (None, "{}", &[]),
        // At its default a user's key changes nothing.
        (None, r#"{"sandbox": {"required": false}}"#, &[]),
        (None, r#"{"sandbox": {"required": true}}"#, &required),
        (
            None,
            r#"{"sandbox": {"allow_unprotected_auto": false}}"#,
            &auto,
        ),
        // A firm's own value counts, even at the default.
        (Some(r#"{"sandbox": {"required": false}}"#), "{}", &required),
        (
            Some(r#"{"sandbox": {"required": true, "allow_unprotected_auto": false}}"#),
            "{}",
            &both,
        ),
        // A policy that cannot be read locks the keys off, but no one set them.
        (Some("{ not json"), "{}", &[]),
    ];
    for (row, (policy, settings, expected)) in rows.into_iter().enumerate() {
        assert_eq!(
            set_keys(&load(
                &format!("protection_ignored_{row}"),
                policy,
                settings
            )),
            expected,
            "{policy:?} {settings}"
        );
    }
}

/// A log capture: every line written under it, as text.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The warning lines `each` logged.
fn logged(each: impl FnOnce()) -> Vec<String> {
    let captured = Captured::default();
    let sink = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .with_max_level(tracing::Level::TRACE)
        .finish();
    tracing::dispatcher::with_default(&tracing::Dispatch::new(subscriber), each);
    let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
    text.lines()
        .filter(|line| line.contains("ignored in this version"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_warning_goes_out_once_per_change_and_only_when_a_key_is_set() {
    let mut host = ProtectionHost::new(None, observe(None));
    let lines = logged(|| {
        host.warn_ignored(Vec::new());
        host.warn_ignored(vec!["sandbox.required"]);
        // The same keys again (another launch, another settings change): nothing more.
        host.warn_ignored(vec!["sandbox.required"]);
        host.warn_ignored(vec!["sandbox.required", "sandbox.allow_unprotected_auto"]);
        host.warn_ignored(Vec::new());
        host.warn_ignored(Vec::new());
    });
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].contains("WARN"), "{lines:?}");
    assert!(
        lines[0].contains(
            "sandbox.required is set but ignored in this version: the Safe workspace isn't part \
             of this release"
        ),
        "{lines:?}"
    );
    assert!(
        lines[1].contains("sandbox.required and sandbox.allow_unprotected_auto are set"),
        "{lines:?}"
    );
}

#[test]
fn with_a_sandbox_wired_no_warning_goes_out() {
    let protected = || Detection {
        tier: Tier::A,
        reason: None,
    };
    let sandbox = Sandbox::new(Arc::new(protected), None);
    let first = observe(Some(&sandbox));
    let mut host = ProtectionHost::new(Some(sandbox), first);
    assert!(host.sandbox_wired());
    let lines = logged(|| host.warn_ignored(vec!["sandbox.required"]));
    assert_eq!(lines, Vec::<String>::new());
}
