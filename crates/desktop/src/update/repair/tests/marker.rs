//! The `repair-pending` marker: read and deleted whatever it holds, fresh only for this version
//! within 30 minutes.
#![expect(
    clippy::disallowed_methods,
    reason = "test set-up writes and removes files in its scratch folder; no write module here"
)]

use super::super::marker::{FRESH, consume, remove, write};
use super::VERSION;
use crate::test_support::Scratch;

/// When the tests write their markers, in Unix seconds.
const AT: u64 = 1_762_600_000;

/// A fresh marker for this version is consumed as fresh and deleted; consumed again, it is gone.
#[test]
fn a_fresh_marker_is_consumed_and_deleted() {
    let scratch = Scratch::new("repair-marker-fresh", &[]);
    let path = scratch.path().join("repair-pending.json");
    write(&path, VERSION, AT).unwrap();
    assert!(consume(&path, VERSION, AT + 60));
    assert!(!path.exists());
    assert!(!consume(&path, VERSION, AT + 60));
    // At the limit it still counts.
    write(&path, VERSION, AT).unwrap();
    assert!(consume(&path, VERSION, AT + FRESH.as_secs()));
}

/// An expired marker, another version's, one from the future and an unreadable one are not
/// fresh, and each is deleted all the same.
#[test]
fn any_other_marker_is_not_fresh_and_is_deleted() {
    let scratch = Scratch::new("repair-marker-stale", &[]);
    let path = scratch.path().join("repair-pending.json");
    for (version, now) in [
        (VERSION, AT + FRESH.as_secs() + 1),
        ("0.4.0", AT + 60),
        (VERSION, AT - 1),
    ] {
        write(&path, version, AT).unwrap();
        assert!(!consume(&path, VERSION, now), "{version} at {now}");
        assert!(!path.exists(), "{version} at {now}");
    }
    for garbage in [&b"not json"[..], b"{}", &vec![b' '; 8 * 1024]] {
        std::fs::write(&path, garbage).unwrap();
        assert!(!consume(&path, VERSION, AT));
        assert!(!path.exists());
    }
}

/// No marker is nothing, and removing one that is not there is fine.
#[test]
fn no_marker_is_nothing() {
    let scratch = Scratch::new("repair-marker-none", &[]);
    let path = scratch.path().join("repair-pending.json");
    assert!(!consume(&path, VERSION, AT));
    remove(&path);
    assert!(!path.exists());
}
