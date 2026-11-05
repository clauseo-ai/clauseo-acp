//! `npm ci` reads the checked tarballs in place through `file:` `resolved` URLs, with no
//! `npm cache add`, and still checks each against the lock's `integrity`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_agents::testing::npm_tarball;
use clauseo_agents::{Arch, FailureKind, InstallState, Os, Platform};
use serde_json::Value;

use crate::support::{A_PACKAGES, Fixture, agent_a};
use crate::write;

/// What a test saw at each npm launch: its Node arguments, the lock in its
/// folder and the files in `downloads/` at that moment.
type Seen = Arc<Mutex<Vec<(Vec<String>, Option<String>, Vec<PathBuf>)>>>;

/// The `file:` URL the install writes for the absolute path `path`.
fn url_of(path: &Path) -> String {
    let text = path.to_str().unwrap();
    if cfg!(windows) {
        format!("file:///{}", text.replace('\\', "/"))
    } else {
        format!("file://{text}")
    }
}

/// The Node arguments when `spec` is an npm run (one of them ends in `npm-cli.js`), else
/// `None`.
fn npm_args(spec: &clauseo_process::LaunchSpec) -> Option<Vec<String>> {
    let args: Vec<String> = spec
        .args()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    args.iter()
        .any(|arg| arg.ends_with("npm-cli.js"))
        .then_some(args)
}

/// The `.tgz` files in `<data>/agents/downloads`.
fn downloaded(data: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(data.join("agents/downloads"))
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "tgz"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn npm_ci_alone_reads_each_checked_tarball_through_a_file_url_with_its_integrity() {
    let fx = Fixture::new("file-urls");
    let seen: Seen = Arc::default();
    let record = Arc::clone(&seen);
    let data = fx.data.path().to_owned();
    let (agents, events) = fx.start_hooked(move |spec| {
        if let Some(args) = npm_args(spec) {
            let lock = std::fs::read_to_string(spec.cwd().join("package-lock.json")).ok();
            record.lock().unwrap_or_else(PoisonError::into_inner).push((
                args,
                lock,
                downloaded(&data),
            ));
        }
    });
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    assert!(matches!(last, InstallState::Ready { .. }), "{last:?}");
    let seen = seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(seen.len(), 1, "one npm run, `npm ci`: {seen:?}");
    let (args, lock, files) = &seen[0];
    // Node runs npm with no V8 background tasks, so it cannot deadlock as it exits
    // (nodejs/node#54918); then `npm-cli.js ci`.
    assert_eq!(args[0], "--single-threaded", "{args:?}");
    assert!(args[1].ends_with("npm-cli.js"), "{args:?}");
    assert_eq!(args[2], "ci");
    for flag in [
        "--offline",
        "--ignore-scripts",
        "--no-bin-links",
        "--allow-file=all",
    ] {
        assert!(args.iter().any(|arg| arg == flag), "{flag} in {args:?}");
    }
    let lock: Value = serde_json::from_str(lock.as_deref().unwrap()).unwrap();
    let snapshot: Value =
        serde_json::from_str(fx.config.snapshot.pins(&agent_a()).unwrap().lock()).unwrap();
    let mut want: Vec<String> = files.iter().map(|file| url_of(file)).collect();
    want.sort();
    let mut got = Vec::new();
    for name in A_PACKAGES {
        let key = format!("node_modules/{name}");
        let entry = &lock["packages"][&key];
        let pinned = &snapshot["packages"][&key];
        assert_eq!(entry["integrity"], pinned["integrity"], "{name}");
        got.push(entry["resolved"].as_str().unwrap().to_owned());
        let mut rest = entry.clone();
        let mut pinned = pinned.clone();
        rest["resolved"] = Value::Null;
        pinned["resolved"] = Value::Null;
        assert_eq!(rest, pinned, "{name}: only resolved differs");
    }
    got.sort();
    // Each entry names a checked download, there when `npm ci` starts.
    assert_eq!(got, want);
    assert_eq!(files.len(), A_PACKAGES.len());
    assert_eq!(lock["packages"][""], snapshot["packages"][""]);
}

#[test]
fn a_tarball_changed_after_its_check_fails_npm_ci_with_eintegrity() {
    let fx = Fixture::new("swapped");
    let data = fx.data.path().to_owned();
    let swaps = Arc::new(Mutex::new(0_usize));
    let count = Arc::clone(&swaps);
    let other = npm_tarball("dep-a", "1.0.0", &[("index.js", b"module.exports = 2;\n")]).unwrap();
    let (agents, events) = fx.start_hooked(move |spec| {
        if npm_args(spec).is_some_and(|args| args.iter().any(|arg| arg == "ci")) {
            // Same names, different bytes, after the download's own check.
            for path in downloaded(&data) {
                write::write(&path, &other).unwrap();
                *count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            }
        }
    });
    events.first_states();
    agents.install(&agent_a());
    // Fired from a thread, so an install that wrongly succeeds is reported, not waited on.
    let timer = fx.timer.clone();
    std::thread::spawn(move || {
        timer.fire_next(Duration::from_secs(2));
        timer.fire_next(Duration::from_secs(8));
    });
    let (_, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("the install succeeded with swapped tarballs: {last:?}");
    };
    assert_eq!(failure.kind, FailureKind::Failed);
    let log = failure.log.to_string();
    assert!(log.contains("npm error code EINTEGRITY"), "{log}");
    assert_eq!(
        *swaps.lock().unwrap_or_else(PoisonError::into_inner),
        3 * A_PACKAGES.len()
    );
    assert_eq!(agents.installed(&agent_a()), None);
    let root = fx.install_root(&agent_a());
    let left: Vec<_> = std::fs::read_dir(&root)
        .map(|entries| entries.flatten().map(|entry| entry.file_name()).collect())
        .unwrap_or_default();
    assert_eq!(left, Vec::<std::ffi::OsString>::new(), "nothing installed");
}

#[test]
fn a_windows_data_folder_on_a_share_fails_at_once_without_a_retry_or_a_download() {
    let mut fx = Fixture::new("share");
    let share = PathBuf::from(r"\\srv\share\Clauseo");
    fx.config.data_dir = share.clone();
    fx.config.platform = Platform::new(Os::Win32, Arch::X64);
    let (agents, events) = fx.start();
    events.first_states();
    agents.install(&agent_a());
    let (_, last) = events.until_end(&agent_a());
    let InstallState::Failed(failure) = last else {
        panic!("not failed: {last:?}");
    };
    assert_eq!(failure.kind, FailureKind::Failed);
    let log = failure.log.to_string();
    assert!(
        log.contains("the data folder is on a network share; npm cannot install from there"),
        "{log}"
    );
    assert_eq!(fx.registry.requests(), []);
    assert_eq!(fx.timer.install_waits(), Vec::<Duration>::new());
    // Nothing was made under the share's name (a relative path off Windows).
    assert!(!share.exists() || cfg!(windows));
}
