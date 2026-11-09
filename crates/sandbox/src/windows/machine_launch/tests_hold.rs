//! The guard beside DCOM's rule (`hold.rs`): no access value gets COM's effective one written
//! first and the deny verified after a settle; a value DCOM moves to `…Old`, or a missing
//! effective descriptor, is degraded; the refresh finds a move as drift and records it; the
//! removal deletes only the access value Clauseo added, byte-identical, and cleans `…Old`.

use super::fake::{FakeStore, machine_access};
use super::hold::{HISTORY_MAX, SETTLE_MS, pack, unpack};
use super::test_sd::{A, GROUP, OTHER_GROUP, ace, descriptor, sid, windows_11, windows_11_aces};
use super::{Change, Guard, LaunchRemoved, LaunchSd, Raw, apply, remove};

/// A machine with no launch value and no access value.
fn bare() -> FakeStore {
    let mut store = FakeStore::new(Raw::Absent);
    store.access = Raw::Absent;
    store
}

#[test]
fn no_access_value_gets_coms_effective_one_then_the_deny_is_verified() {
    let mut store = bare();
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(
        (applied.guard, applied.change, applied.why),
        (Guard::Applied, Change::Added, None)
    );
    // Exactly COM's descriptor, written before the launch value, and recorded.
    assert_eq!(store.access, Raw::Binary(machine_access()));
    assert_eq!(
        store.writes(),
        ["write_backup", "write_access", "write", "write_state"]
    );
    let backup = store.backup.clone().unwrap();
    assert_eq!(backup.access, Some(machine_access()));
    // Read back after each settle, the launch value still there with the deny first.
    let settles = store.calls.iter().filter(|c| **c == "settle").count();
    assert_eq!(settles, SETTLE_MS.len());
    assert_eq!(SETTLE_MS.iter().sum::<u64>(), 1000);
    let last_settle = store.calls.iter().rposition(|c| *c == "settle").unwrap();
    assert!(store.calls[last_settle..].contains(&"read"));
    let Raw::Binary(value) = &store.value else {
        panic!("moved: {:?}", store.old)
    };
    assert!(LaunchSd::parse(value).unwrap().denies_first(&sid(GROUP)));
    assert_eq!(store.old, Raw::Absent);
    // An existing access value is never touched, nor COM asked.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    assert!(!store.calls.contains(&"effective_access"));
    assert_eq!(store.backup.unwrap().access, None);
}

#[test]
fn a_value_dcom_moves_is_degraded_never_applied() {
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.moves = true;
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(applied.guard, Guard::Moved);
    assert!(applied.guard.degraded());
    let why = applied.why.clone().unwrap();
    assert!(why.contains("dcom moved the value"), "{why}");
    assert_eq!(
        store.state.as_deref(),
        Some("degraded: dcom moved the value")
    );
    assert!(
        applied
            .line()
            .unwrap()
            .contains("degraded: dcom moved the value")
    );
    assert_eq!(store.value, Raw::Absent);
}

#[test]
fn coms_effective_descriptor_unread_is_degraded_and_nothing_is_written() {
    let mut store = bare();
    store.effective = Err("CoGetSystemSecurityPermissions failed (0x80004005)".to_owned());
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(
        (applied.guard, applied.change),
        (Guard::AccessUnknown, Change::Unchanged)
    );
    assert!(
        applied
            .why
            .unwrap()
            .contains("CoGetSystemSecurityPermissions")
    );
    assert_eq!(store.writes(), ["write_state"]);
    assert_eq!(
        (store.value.clone(), store.access.clone()),
        (Raw::Absent, Raw::Absent)
    );
    assert_eq!(
        store.state.as_deref(),
        Some("degraded: machine access unknown")
    );
}

#[test]
fn the_refresh_finds_a_move_as_drift_and_records_it_degraded() {
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    assert_eq!(store.state.as_deref(), Some("applied"));
    // Since, the access value went, DCOM moved ours, and COM's own can't be read: degraded.
    store.access = Raw::Absent;
    store.effective = Err("CoGetSystemSecurityPermissions failed (5)".to_owned());
    store.old = std::mem::replace(&mut store.value, Raw::Absent);
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(applied.guard, Guard::AccessUnknown);
    // With the access value back but DCOM still moving it: moved, degraded.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    store.value = Raw::Binary(windows_11());
    store.moves = true;
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(applied.guard, Guard::Moved);
    assert_eq!(
        store.state.as_deref(),
        Some("degraded: dcom moved the value")
    );
    // Once it holds again, the state goes back to applied.
    store.moves = false;
    store.value = Raw::Binary(windows_11());
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::Applied);
    assert_eq!(store.state.as_deref(), Some("applied"));
}

#[test]
fn removal_deletes_the_added_access_value_only_while_byte_identical() {
    let mut store = bare();
    apply(&mut store, &sid(GROUP));
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Deleted));
    assert_eq!(
        (store.value.clone(), store.access.clone()),
        (Raw::Absent, Raw::Absent)
    );
    assert!(store.calls.contains(&"delete_access"));
    // Changed since: left.
    let mut store = bare();
    apply(&mut store, &sid(GROUP));
    let changed = descriptor(&[ace(A, 0, 0x3, "S-1-1-0")], false);
    store.access = Raw::Binary(changed.clone());
    remove(&mut store, &sid(GROUP)).unwrap();
    assert_eq!(store.access, Raw::Binary(changed));
    assert!(!store.calls.contains(&"delete_access"));
    // Still identical, but a launch value is left that DCOM would then move: left.
    let mut store = bare();
    apply(&mut store, &sid(GROUP));
    let other = descriptor(&[ace(A, 0, 0xB, "S-1-5-32-545")], false);
    store.value = Raw::Binary(other.clone());
    remove(&mut store, &sid(GROUP)).unwrap();
    assert_eq!(store.access, Raw::Binary(machine_access()));
    assert_eq!(store.value, Raw::Binary(other));
    // Never added: never deleted.
    let mut store = FakeStore::new(Raw::Absent);
    apply(&mut store, &sid(GROUP));
    remove(&mut store, &sid(GROUP)).unwrap();
    assert_eq!(store.access, Raw::Binary(machine_access()));
}

#[test]
fn removal_takes_clauseos_bytes_out_of_old_and_leaves_anyone_elses() {
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.moves = true;
    apply(&mut store, &sid(GROUP));
    let Raw::Binary(ours) = store.old.clone() else {
        panic!("not moved")
    };
    assert_eq!(store.backup.as_ref().map(|b| b.written.clone()), Some(ours));
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Restored));
    assert_eq!(store.value, Raw::Binary(windows_11()));
    assert_eq!(store.old, Raw::Absent);
    assert!(store.calls.contains(&"delete_old"));
    assert_eq!((store.backup.clone(), store.state.clone()), (None, None));
    // Someone else's `…Old` stays.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    store.old = Raw::Binary(windows_11());
    remove(&mut store, &sid(GROUP)).unwrap();
    assert_eq!(store.old, Raw::Binary(windows_11()));
    assert!(!store.calls.contains(&"delete_old"));
}

#[test]
fn a_group_policy_access_value_means_nothing_is_copied() {
    let mut store = bare();
    store.access_policy = true;
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(
        (applied.guard, applied.change),
        (Guard::PolicyOverrides, Change::Unchanged)
    );
    assert!(
        applied
            .why
            .unwrap()
            .contains("Group Policy MachineAccessRestriction")
    );
    assert!(!store.calls.contains(&"effective_access"));
    assert_eq!(store.writes(), ["write_state"]);
    assert_eq!(
        (store.value.clone(), store.access.clone()),
        (Raw::Absent, Raw::Absent)
    );
    assert_eq!(store.state.as_deref(), Some("degraded: group policy"));
    // A local access value already there: the policy changes nothing, nothing is copied.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.access_policy = true;
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::Applied);
}

#[test]
fn removal_takes_any_earlier_write_out_of_old() {
    // Three writes: the first, then two put-backs after drift, each a different value.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    let Raw::Binary(first) = store.value.clone() else {
        panic!("not written")
    };
    for extra in [0xB, 0x3] {
        let mut aces = windows_11_aces();
        aces.push(ace(A, 0, extra, OTHER_GROUP));
        store.value = Raw::Binary(descriptor(&aces, false));
        assert_eq!(apply(&mut store, &sid(GROUP)).change, Change::PutBack);
    }
    let history = store.backup.clone().unwrap().history;
    assert_eq!(history.len(), 3);
    assert_eq!(history.first(), Some(&first));
    assert_ne!(
        store.backup.as_ref().map(|b| b.written.clone()),
        Some(first.clone())
    );
    // DCOM moved the first write to `…Old`: still Clauseo's, so it goes.
    store.old = Raw::Binary(first);
    remove(&mut store, &sid(GROUP)).unwrap();
    assert_eq!(store.old, Raw::Absent);
    // The history keeps the last few, each once, and survives the registry's packing.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    let masks = [0x1, 0x3, 0x5, 0x7, 0x9, 0xB, 0xD, 0xF, 0x11, 0x13];
    assert_eq!(masks.len(), HISTORY_MAX + 2);
    for mask in masks {
        let mut aces = windows_11_aces();
        aces.push(ace(A, 0, mask, OTHER_GROUP));
        store.value = Raw::Binary(descriptor(&aces, false));
        apply(&mut store, &sid(GROUP));
    }
    let backup = store.backup.clone().unwrap();
    assert_eq!(backup.history.len(), HISTORY_MAX);
    assert_eq!(backup.history.last(), Some(&backup.written));
    assert_eq!(unpack(&pack(&backup.history)), backup.history);
    let mut cut = pack(&backup.history);
    cut.truncate(cut.len() - 1);
    assert_eq!(unpack(&cut).len(), HISTORY_MAX - 1);
}

#[test]
fn removal_restores_the_original_when_the_value_is_gone() {
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.moves = true;
    apply(&mut store, &sid(GROUP));
    assert_eq!(store.value, Raw::Absent);
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Restored));
    assert_eq!(store.value, Raw::Binary(windows_11()));
    // Deleted since (no move): the original goes back too.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    store.value = Raw::Absent;
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Restored));
    assert_eq!(store.value, Raw::Binary(windows_11()));
    // No original: nothing to put back.
    let mut store = bare();
    store.moves = true;
    apply(&mut store, &sid(GROUP));
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Nothing));
    assert_eq!(store.value, Raw::Absent);
}
