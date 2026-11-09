//! `--remove` and the unclean-session markers: the owner's removal clears them, as the owner,
//! before the record (which takes the stop's and the sweep's values with it); an
//! administrator's leaves them, since it deletes nothing in the owner's folders.

use super::fake::OTHER_OWNER;
use super::remove::pool;
use super::*;
use crate::windows::runner::marker::{
    MarkerState, Outer, unclean_sessions, write_marker, write_marker_at,
};
use crate::windows::setup::record_values::{
    SETUP_VALUES, STOP_COMPLETED, STOP_NEXT, STOP_SEQUENCE, SWEEP_COMPLETED, SWEEP_NEXT,
    SWEEP_SEQUENCE,
};

/// Two markers in the machine's data folder: one open, one awaiting a stop.
fn leave_markers(machine: &remove_fake::Remover) {
    let data = machine.m.data_dir();
    machine.m.dir.dir("data/sandbox/run");
    let pool_user = Sid::parse("S-1-5-21-1-2-3-1001").unwrap();
    let outer = Outer { pid: 7, created: 1 };
    write_marker(&data, "00000000000000aa", &pool_user, None, outer).unwrap();
    let waiting = write_marker(&data, "00000000000000bb", &pool_user, None, outer).unwrap();
    let state = MarkerState::AwaitingStop { stop: 3 };
    write_marker_at(&waiting, Some(&pool_user), None, state).unwrap();
    assert_eq!(unclean_sessions(&data).unwrap().len(), 2);
}

#[test]
fn the_owners_removal_clears_the_markers_before_the_record() {
    let (mut machine, mut acl) = pool("remove-markers", Some(owner()));
    leave_markers(&machine);
    remove(&mut machine, &mut acl, false).unwrap();
    assert_eq!(unclean_sessions(&machine.m.data_dir()).unwrap(), []);
    let lines = machine.lines();
    let at = |line: &str| lines.iter().position(|l| l == line).unwrap();
    assert!(at("delete_group 1000") < at("clear_markers"));
    assert!(at("clear_markers") < at("delete_record"));
    // The record goes with the set-up's values: the stop's and the sweep's among them.
    for name in [
        STOP_COMPLETED,
        STOP_SEQUENCE,
        STOP_NEXT,
        SWEEP_COMPLETED,
        SWEEP_SEQUENCE,
        SWEEP_NEXT,
    ] {
        assert!(SETUP_VALUES.contains(&name), "{name}");
    }
    assert_eq!(machine.m.state.record, None);
}

#[test]
fn a_marker_that_wont_go_stops_the_removal_with_the_record_kept() {
    let (mut machine, mut acl) = pool("remove-markers-fail", Some(owner()));
    leave_markers(&machine);
    let full = remove::full_removal();
    let at = full.iter().position(|l| l == "clear_markers").unwrap();
    machine.fail_at.set(Some(at));
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (RemoveStep::Markers, SetupExit::RemovedPartly)
    );
    assert!(machine.m.state.record.is_some());
    assert_eq!(unclean_sessions(&machine.m.data_dir()).unwrap().len(), 2);
    // The next run finishes, and the markers go.
    remove(&mut machine, &mut acl, false).unwrap();
    assert_eq!(unclean_sessions(&machine.m.data_dir()).unwrap(), []);
}

#[test]
fn an_administrators_removal_leaves_the_owners_markers() {
    let other = Sid::parse(OTHER_OWNER).unwrap();
    let (mut machine, mut acl) = pool("remove-markers-admin", Some(other));
    leave_markers(&machine);
    remove(&mut machine, &mut acl, true).unwrap();
    assert!(!machine.lines().iter().any(|l| l == "clear_markers"));
    // Stale now: they name pool SIDs no record lists, so the next set-up's readers drop them.
    assert_eq!(unclean_sessions(&machine.m.data_dir()).unwrap().len(), 2);
    assert_eq!(machine.m.state.record, None);
}
