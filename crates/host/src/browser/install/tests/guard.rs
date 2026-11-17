//! The guardrails inside a run: never older, the record, `behind`, the size cap, the offline
//! wait, and old copies removed only when no session runs them.

use std::path::Path;
use std::time::Duration;

use clauseo_protocol::{BrowserInstallFailure, BrowserInstallState};

use super::super::fresh::{self, StableRecord};
use super::super::prune::{self, InUse};
use super::super::{Cancel, MAX_ZIP_BYTES, Outcome, run, run_until_online};
use super::support::{Fake, Length, MAC_BINARY, VERSION, google_url, versions};
use super::{browsers, fails_with, job};
use crate::browser::chrome::Platform;
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// Makes a copy of `version` with its binary in `dir`'s browsers folder.
fn copy(dir: &Path, version: &str) {
    let path = MAC_BINARY.split('/').fold(
        dir.join("app")
            .join("browsers")
            .join(format!("chrome-{version}")),
        |path, part| path.join(part),
    );
    write::create_dir_all(path.parent().unwrap()).unwrap();
    write::write_file(&path, b"chrome").unwrap();
}

/// The run's outcome and states on Apple silicon.
fn outcome(
    dir: &Path,
    tools: &Fake,
) -> (Result<Outcome, super::Failure>, Vec<BrowserInstallState>) {
    let mut states = Vec::new();
    let result = run(
        &job(dir, Platform::MacArm64),
        tools,
        &Cancel::new(),
        &mut |state| {
            states.push(state);
        },
    );
    (result, states)
}

/// Stable below the copy installed is refused and nothing downloads; Stable equal to it is
/// current, downloads nothing, and still records what it saw.
#[test]
fn the_run_never_downgrades_and_skips_a_current_copy() {
    let dir = TempDir::new("cft-guard-down");
    copy(dir.path(), "999.0.0.0");
    let tools = Fake::working();
    let (result, _) = outcome(dir.path(), &tools);
    let failure = result.unwrap_err();
    assert_eq!(failure.code, BrowserInstallFailure::HostRefused);
    assert!(
        failure.message.contains("never downgraded"),
        "{}",
        failure.message
    );
    assert_eq!(tools.calls(), ["fetch_versions"]);

    let dir = TempDir::new("cft-guard-current");
    copy(dir.path(), VERSION);
    let tools = Fake::working();
    let (result, states) = outcome(dir.path(), &tools);
    assert_eq!(result.unwrap(), Outcome::Current);
    assert_eq!(states, Vec::<BrowserInstallState>::new());
    assert_eq!(tools.calls(), ["fetch_versions"]);
    let record = fresh::read_record(&dir.path().join("app").join("browsers")).unwrap();
    assert_eq!(record.stable, VERSION);
}

/// A Stable below the floor is refused before anything downloads.
#[test]
fn a_stable_below_the_floor_is_refused() {
    let old = Fake {
        versions: versions("131.0.6778.85", &google_url("131.0.6778.85")),
        ..Fake::working()
    };
    let failure = fails_with("cft-guard-floor", &old, BrowserInstallFailure::HostRefused);
    assert!(failure.message.contains("floor"), "{}", failure.message);
    assert_eq!(old.calls(), ["fetch_versions"]);
}

/// Over a copy more than one release behind, the download says `behind`; then the old copy
/// goes, since no session runs it.
#[test]
fn an_update_of_a_copy_two_releases_behind_says_behind_and_prunes_it() {
    let dir = TempDir::new("cft-guard-behind");
    copy(dir.path(), "158.0.0.1");
    let browsers_dir = dir.path().join("app").join("browsers");
    let seen = StableRecord {
        stable: "158.0.0.5".to_owned(),
        previous: Some("158.0.0.4".to_owned()),
        checked_at: 1_700_000_000,
    };
    fresh::write_record(&browsers_dir, &seen).unwrap();
    let tools = Fake::working();
    let (result, states) = outcome(dir.path(), &tools);
    assert!(matches!(result.unwrap(), Outcome::Installed(_)));
    assert!(
        states
            .iter()
            .any(|state| matches!(state, BrowserInstallState::Downloading { behind: true, .. }))
    );
    let removed = prune::prune(&browsers_dir, Path::new(MAC_BINARY), &InUse::new());
    assert_eq!(removed, [browsers_dir.join("chrome-158.0.0.1")]);
    assert_eq!(browsers(dir.path()), [format!("chrome-{VERSION}")]);
}

/// An update over a copy that is Stable's release before is not `behind`.
#[test]
fn an_update_one_release_behind_is_not_behind() {
    let dir = TempDir::new("cft-guard-one");
    copy(dir.path(), "158.0.0.1");
    let browsers_dir = dir.path().join("app").join("browsers");
    let seen = StableRecord {
        stable: "158.0.0.1".to_owned(),
        previous: None,
        checked_at: 1_700_000_000,
    };
    fresh::write_record(&browsers_dir, &seen).unwrap();
    let (result, states) = outcome(dir.path(), &Fake::working());
    assert!(matches!(result.unwrap(), Outcome::Installed(_)));
    assert!(
        states
            .iter()
            .all(|state| !matches!(state, BrowserInstallState::Downloading { behind: true, .. }))
    );
}

/// A zip whose `Content-Length` is over the cap is refused even when the size matches it.
#[test]
fn a_zip_over_the_cap_is_refused() {
    let huge = Fake {
        length: Length::Wrong(MAX_ZIP_BYTES + 1),
        ..Fake::working()
    };
    let failure = fails_with("cft-guard-cap", &huge, BrowserInstallFailure::SizeMismatch);
    assert!(failure.message.contains("cap"), "{}", failure.message);
}

/// Old copies go only when no session runs them: a noted session whose run folder is there
/// keeps its copy; once that folder is gone, the next prune removes it. The highest stays.
#[test]
fn old_copies_go_only_when_unused() {
    let dir = TempDir::new("cft-guard-prune");
    copy(dir.path(), "158.0.0.1");
    copy(dir.path(), "158.0.0.2");
    copy(dir.path(), "158.0.0.3");
    let browsers_dir = dir.path().join("app").join("browsers");
    let run_root = dir.path().join("run").join("s1");
    write::create_dir_all(&run_root).unwrap();
    let in_use = InUse::new();
    let binary = browsers_dir.join("chrome-158.0.0.2").join("x");
    let found = in_use.search(&browsers_dir, &run_root, || Some(binary.clone()));
    assert_eq!(found, Some(binary));
    let removed = prune::prune(&browsers_dir, Path::new(MAC_BINARY), &in_use);
    assert_eq!(removed, [browsers_dir.join("chrome-158.0.0.1")]);
    write::remove_tree(&run_root).unwrap();
    let removed = prune::prune(&browsers_dir, Path::new(MAC_BINARY), &in_use);
    assert_eq!(removed, [browsers_dir.join("chrome-158.0.0.2")]);
    assert_eq!(browsers(dir.path()), ["chrome-158.0.0.3"]);
}

/// The lawyer's download waits out a dropped connection: `Offline` between tries, then it goes
/// on. A cancel during the wait ends it as `cancelled`.
#[test]
fn the_lawyers_download_waits_while_offline() {
    let dir = TempDir::new("cft-guard-offline");
    let tools = Fake::working();
    tools.offline.store(2, std::sync::atomic::Ordering::SeqCst);
    let mut states = Vec::new();
    let result = run_until_online(
        &job(dir.path(), Platform::MacArm64),
        &tools,
        &Cancel::new(),
        &mut |state| states.push(state),
        Duration::from_millis(1),
    );
    assert!(matches!(result.unwrap(), Outcome::Installed(_)));
    let offline = states
        .iter()
        .filter(|state| **state == BrowserInstallState::Offline)
        .count();
    assert_eq!(offline, 2);
    assert_eq!(&tools.calls()[..3], ["fetch_versions"; 3]);

    let dir = TempDir::new("cft-guard-offline-cancel");
    let tools = Fake::working();
    tools
        .offline
        .store(u32::MAX, std::sync::atomic::Ordering::SeqCst);
    let cancel = Cancel::new();
    let mut seen = 0;
    let result = run_until_online(
        &job(dir.path(), Platform::MacArm64),
        &tools,
        &cancel,
        &mut |_| {
            seen += 1;
            if seen == 3 {
                cancel.cancel();
            }
        },
        Duration::from_millis(1),
    );
    assert_eq!(result.unwrap_err().code, BrowserInstallFailure::Cancelled);
}
