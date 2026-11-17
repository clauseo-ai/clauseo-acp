//! The guardrails: the floor, never older, the record of Stables seen, and "more than one
//! release behind".

use super::{
    CFT_FLOOR, STALE_AFTER_SECS, StableRecord, at_or_above_floor, check_install, installed,
    observe, read_record, too_far_behind, write_record,
};
use crate::pipeline::tests::support::TempDir;
use crate::write;
use clauseo_protocol::BrowserInstallFailure;

const DAY: i64 = 24 * 60 * 60;

fn record(stable: &str, previous: Option<&str>, checked_at: i64) -> StableRecord {
    StableRecord {
        stable: stable.to_owned(),
        previous: previous.map(str::to_owned),
        checked_at,
    }
}

/// The floor is a dotted version this release holds; nothing below it passes.
#[test]
fn the_floor_is_a_version_and_refuses_what_is_below() {
    assert!(crate::browser::chrome::version(CFT_FLOOR).is_some());
    assert!(at_or_above_floor(CFT_FLOOR));
    assert!(at_or_above_floor("999.0.0.0"));
    assert!(!at_or_above_floor("131.0.6778.85"));
    assert!(!at_or_above_floor("not a version"));
    let failure = check_install("131.0.6778.85", &[]).unwrap_err();
    assert_eq!(failure.code, BrowserInstallFailure::HostRefused);
    assert!(failure.message.contains("floor"), "{}", failure.message);
}

/// Stable older than the copy installed is refused; the same is nothing to do; newer downloads.
#[test]
fn never_downgrade() {
    let installed = ["158.0.1.2".to_owned(), "157.9.0.0".to_owned()];
    let failure = check_install("158.0.1.1", &installed).unwrap_err();
    assert_eq!(failure.code, BrowserInstallFailure::HostRefused);
    assert!(failure.message.contains("never downgraded"));
    assert!(!check_install("158.0.1.2", &installed).unwrap());
    assert!(check_install("158.0.1.10", &installed).unwrap());
    assert!(check_install("158.0.1.2", &[]).unwrap());
}

/// A new Stable moves the old one to `previous`; an older or equal one only refreshes the time;
/// a new Stable after the record went unrefreshed forgets the old one.
#[test]
fn the_record_follows_stable_and_never_goes_back() {
    let first = observe(None, "158.0.0.1", 10);
    assert_eq!(first, record("158.0.0.1", None, 10));
    let next = observe(Some(&first), "158.0.0.2", 10 + DAY);
    assert_eq!(next, record("158.0.0.2", Some("158.0.0.1"), 10 + DAY));
    let back = observe(Some(&next), "158.0.0.1", 10 + 2 * DAY);
    assert_eq!(back, record("158.0.0.2", Some("158.0.0.1"), 10 + 2 * DAY));
    let long_after = observe(
        Some(&back),
        "159.0.0.1",
        10 + 2 * DAY + STALE_AFTER_SECS + 1,
    );
    assert_eq!(long_after.stable, "159.0.0.1");
    assert_eq!(long_after.previous, None);
}

/// Stable or the release before it may run; anything older may not, nor anything older than
/// Stable when the release before is unknown. No record refuses nothing.
#[test]
fn more_than_one_release_behind_is_refused() {
    let known = record("158.0.0.3", Some("158.0.0.2"), 0);
    assert!(!too_far_behind("158.0.0.3", Some(&known)));
    assert!(!too_far_behind("158.0.0.4", Some(&known)));
    assert!(!too_far_behind("158.0.0.2", Some(&known)));
    assert!(too_far_behind("158.0.0.1", Some(&known)));
    assert!(too_far_behind("157.9.9.9", Some(&known)));
    let unknown = record("158.0.0.3", None, 0);
    assert!(!too_far_behind("158.0.0.3", Some(&unknown)));
    assert!(too_far_behind("158.0.0.2", Some(&unknown)));
    assert!(!too_far_behind("100.0.0.0", None));
    assert!(too_far_behind("x", Some(&known)));
}

/// The record round-trips through its file; a broken file reads as none; the copies list only
/// real `chrome-<version>` folders, highest last.
#[test]
fn the_record_file_and_the_copies_on_disk() {
    let dir = TempDir::new("cft-fresh-disk");
    let browsers = dir.path().join("browsers");
    write::create_dir_all(&browsers).unwrap();
    assert_eq!(read_record(&browsers), None);
    let saved = record("158.0.0.3", Some("158.0.0.2"), 42);
    write_record(&browsers, &saved).unwrap();
    assert_eq!(read_record(&browsers), Some(saved));
    write::write_file(&browsers.join(super::RECORD_FILE), b"{not json").unwrap();
    assert_eq!(read_record(&browsers), None);
    for name in ["chrome-158.0.0.10", "chrome-158.0.0.9", "chrome-x", "other"] {
        write::create_dir_all(&browsers.join(name)).unwrap();
    }
    write::write_file(&browsers.join("chrome-200.0.0.0"), b"a file").unwrap();
    let versions: Vec<String> = installed(&browsers).into_iter().map(|(v, _)| v).collect();
    assert_eq!(versions, ["158.0.0.9", "158.0.0.10"]);
}
