//! The floor record: a stored pair that stops verifying leaves no list until one at or above the
//! floor is there, never the lower bundled copy; and the clock set back under the floor leaves
//! none either.

use super::{MANIFEST_FILE, at, check, put, source};
use crate::browser::pins::floor::{self, FLOOR_FILE, Floor};
use crate::browser::pins::{accept, store};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// Stores the fixture `name` as the refresh does: the floor record first, then the pair.
fn store_fixture(folder: &std::path::Path, name: &str) {
    let (body, signature) = super::fixture(name);
    store(folder, &check(name).unwrap(), &body, &signature).unwrap();
}

#[test]
fn a_store_writes_the_floor_record_and_only_raises_it() {
    let dir = TempDir::new("pins-floor-raise");
    let stored = dir.path().join("stored");
    store_fixture(&stored, "pins-newer");
    let newer = check("pins-newer").unwrap();
    assert_eq!(floor::read(&stored), Ok(Some(Floor::of(&newer))));
    // A lower one never lowers it, even if asked.
    floor::raise(&stored, &check("pins").unwrap()).unwrap();
    assert_eq!(floor::read(&stored), Ok(Some(Floor::of(&newer))));
    // A heartbeat of a higher one raises it.
    floor::raise(&stored, &check("pins-ahead").unwrap()).unwrap();
    assert_eq!(
        floor::read(&stored),
        Ok(Some(Floor::of(&check("pins-ahead").unwrap())))
    );
}

#[test]
fn a_stored_pair_that_fails_to_verify_leaves_no_list_until_one_at_the_floor_arrives() {
    let now = at("2026-11-09T00:00:00Z");
    let dir = TempDir::new("pins-floor-broken");
    let stored = dir.path().join("stored");
    put(&dir.path().join("bundled"), "pins");
    store_fixture(&stored, "pins-newer");
    let source = source(&dir);
    assert_eq!(source.newest(now).unwrap().version(), "157.0.8103.50");
    // The stored pair breaks: its body no longer matches its signature.
    let (older, _) = super::fixture("pins-older");
    write::write_file(&stored.join(MANIFEST_FILE), &older).unwrap();
    assert_eq!(source.newest(now), None, "not the lower bundled copy");
    // Gone altogether: the same.
    write::remove_file(&stored.join(MANIFEST_FILE)).unwrap();
    assert_eq!(source.newest(now), None);
    // The refresh judges against the record too: a replay below it is refused, though above
    // the bundled copy that alone verifies now.
    let record = source.floor_record().unwrap();
    let bundled = source.floor().unwrap();
    assert!(!accept(
        &check("pins-heartbeat").unwrap(),
        Some(&bundled),
        record.as_ref()
    ));
    // The floor's own manifest again is taken, and the list is back.
    assert!(accept(
        &check("pins-newer").unwrap(),
        Some(&bundled),
        record.as_ref()
    ));
    store_fixture(&stored, "pins-newer");
    assert_eq!(source.newest(now).unwrap().version(), "157.0.8103.50");
}

#[test]
fn a_floor_record_dated_ahead_of_the_clock_leaves_no_list() {
    let dir = TempDir::new("pins-floor-ahead");
    let stored = dir.path().join("stored");
    put(&dir.path().join("bundled"), "pins");
    store_fixture(&stored, "pins-ahead");
    // The stored pair breaks; the floor (published 2026-11-20) is ahead of the 18th.
    write::remove_file(&stored.join(MANIFEST_FILE)).unwrap();
    assert_eq!(source(&dir).newest(at("2026-11-18T00:00:00Z")), None);
}

#[test]
fn an_unreadable_floor_record_leaves_no_list_and_a_store_rewrites_it() {
    let now = at("2026-11-09T00:00:00Z");
    let dir = TempDir::new("pins-floor-unreadable");
    let stored = dir.path().join("stored");
    store_fixture(&stored, "pins");
    write::write_file(&stored.join(FLOOR_FILE), b"{").unwrap();
    let source = source(&dir);
    assert_eq!(source.newest(now), None);
    floor::raise(&stored, &check("pins").unwrap()).unwrap();
    assert_eq!(source.newest(now).unwrap().version(), "157.0.8103.38");
}
