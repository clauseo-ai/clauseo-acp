//! The shared list in use: offers under the rollback rule, readers keeping their copy.

use std::sync::Arc;

use clauseo_blocklist::{BlockList, Current, load};

use crate::fixtures::{LIST, NEWER, OLDER, generated, key_a};

#[test]
fn an_offer_applies_only_when_newer_and_a_reader_keeps_its_copy() {
    let current = Current::new(BlockList::empty());
    assert!(current.first_unloaded_notice());
    assert!(!current.first_unloaded_notice());
    assert!(current.offer(load(LIST.gz, LIST.minisig, &[key_a()]).unwrap(), None));
    let held = current.get();
    assert!(!current.offer(load(OLDER.gz, OLDER.minisig, &[key_a()]).unwrap(), None));
    assert!(!current.offer(load(LIST.gz, LIST.minisig, &[key_a()]).unwrap(), None));
    assert!(current.offer(load(NEWER.gz, NEWER.minisig, &[key_a()]).unwrap(), None));
    assert_eq!(current.generated(), Some(generated("2026-11-07T12:00:00Z")));
    assert!(current.get().lookup("newer.test").is_some());
    // The reader that took the list before the swap still has it, whole.
    assert_eq!(held.len(), 6);
    assert!(held.lookup("newer.test").is_none());
    assert_eq!(Arc::strong_count(&held), 1);
    assert!(!current.first_unloaded_notice());
}

#[test]
fn an_offer_older_than_the_bundled_snapshot_is_refused() {
    let current = Current::default();
    let bundled = Some(generated("2026-11-07T06:00:00Z"));
    assert!(!current.offer(load(OLDER.gz, OLDER.minisig, &[key_a()]).unwrap(), bundled));
    assert!(current.get().is_unloaded());
    assert!(!current.offer(BlockList::empty(), None));
}
