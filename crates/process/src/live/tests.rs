//! The slot's count, with the gate open (the closed gate is `tests/close_spawns.rs`, a binary
//! of its own, since it never reopens).

use super::{Live, live_handles};

/// The count rises when the slot is taken, before `spawn_in_tree` builds any command, and
/// falls when it drops (a failed start, or the handle's drop).
#[test]
fn a_slot_counts_from_its_taking_until_its_drop() {
    let before = live_handles();
    let slot = Live::enter().unwrap();
    assert_eq!(live_handles(), before + 1);
    drop(slot);
    assert_eq!(live_handles(), before);
}
