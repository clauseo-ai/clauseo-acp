//! The lock-cause lines through the host's real log file.

use std::path::Path;
use std::time::Duration;

use clauseo_settings::policy::file;
use clauseo_settings::{Effective, Error, Locations, PolicyRead, PolicySource, SystemFiles};

use super::log;
use crate::log_file::{FileSink, build};
use crate::pipeline::tests::support::TempDir;

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

fn load(dir: &TempDir, policy: &'static str) -> Effective {
    Effective::load(
        &Locations::under_root(&dir.path().join("data")),
        &SystemFiles,
        &[&Text(policy)],
    )
}

/// The host log's lines after `each` ran under it.
fn logged(dir: &TempDir, each: impl FnOnce()) -> Vec<String> {
    let logs = dir.path().join("logs");
    let (dispatch, guard) = build(
        FileSink::new(logs.clone()),
        100_000,
        Box::new(|| Duration::ZERO),
    )
    .unwrap();
    tracing::dispatcher::with_default(&dispatch, each);
    drop(guard);
    std::fs::read_to_string(logs.join("host.log"))
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("a policy locked settings"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_broken_policy_logs_its_cause_once_and_a_changed_cause_logs_again() {
    let dir = TempDir::new("lock-causes");
    let misspelt = r#"{"skill": {"extra": "off"}}"#;
    let duplicate = r#"{"skills": {"extra": "off"},
        "skills": {"enabled": {}}}"#;
    let mut last = Vec::new();
    let lines = logged(&dir, || {
        // Start-up, then a reload of the same broken policy: one line.
        log(&mut last, &load(&dir, misspelt));
        log(&mut last, &load(&dir, misspelt));
        // A changed cause: one new line.
        log(&mut last, &load(&dir, duplicate));
        log(&mut last, &load(&dir, duplicate));
        // A plain policy value is no cause: nothing.
        log(&mut last, &load(&dir, r#"{"ui": {"theme": "dark"}}"#));
    });
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert!(
        lines[0].contains("cause=\"misnamed_section\""),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains("source=\"policy.json\""), "{}", lines[0]);
    assert!(
        lines[0].contains("section=\"skill\" place=1"),
        "{}",
        lines[0]
    );
    assert!(lines[1].contains("cause=\"duplicate_key\""), "{}", lines[1]);
    assert!(lines[1].contains("place=2"), "{}", lines[1]);
    for line in &lines {
        assert!(!line.contains("/etc") && !line.contains("off"), "{line}");
    }
    assert_eq!(last, []);
}
