//! The pool group's "Deny log on through Remote Desktop Services" (S15 W1): granted by every
//! set-up once the group is made or adopted, failing at its own step; taken off by the rollback
//! before a group this run made is deleted; taken off by `--remove` before the group goes, a
//! failure there stopping the run at its own step.

use std::collections::BTreeSet;

use super::fake::{FakeAcl, FakeMachine, State};
use super::remove::{assert_all_gone, full_removal, pool};
use super::*;

/// The SIDs `state` denies Remote Desktop logon.
fn denied(state: &State) -> Vec<Sid> {
    state.remote_denied.iter().cloned().collect()
}

#[test]
fn a_fresh_set_up_denies_the_group_it_made_right_after_making_it() {
    let mut machine = FakeMachine::new("setup-rdp-fresh");
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert!(report.group_made);
    assert_eq!(denied(&machine.state), [sid(1000)]);
    let made = machine
        .calls
        .iter()
        .position(|c| c == "create_group")
        .unwrap();
    assert_eq!(
        machine.calls.get(made + 1).map(String::as_str),
        Some("deny_remote_logon 1000")
    );
}

#[test]
fn a_re_run_denies_the_adopted_group_again_repairing_a_deny_taken_off() {
    let mut machine = FakeMachine::new("setup-rdp-rerun");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.state.remote_denied = BTreeSet::new();
    let from = machine.calls.len();
    let report = set_up_fresh(&mut machine, &mut acl);
    assert!(!report.group_made);
    assert_eq!(denied(&machine.state), [sid(1000)]);
    let rest = machine.calls_since(from);
    let adopted = rest.iter().position(|c| *c == "group").unwrap();
    assert_eq!(
        rest.get(adopted + 1).copied(),
        Some("deny_remote_logon 1000")
    );
    assert!(
        !rest.iter().any(|c| c.starts_with("create_group")),
        "{rest:?}"
    );
}

#[test]
fn a_failed_deny_fails_at_its_step_and_rolls_back_the_group_it_made() {
    let mut machine = FakeMachine::new("setup-rdp-fails");
    machine.fail("deny_remote_logon 1000");
    let before = machine.state.clone();
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::RemoteLogon);
    assert_eq!(failure.exit, SetupExit::Failed);
    assert!(failure.rolled_back, "{}", failure.cause);
    assert!(failure.cause.contains("injected"), "{}", failure.cause);
    assert_eq!(machine.state, before);
    assert!(!machine.calls.iter().any(|c| c.starts_with("create_user")));
}

#[test]
fn a_failed_deny_on_a_re_run_fails_at_its_step_and_keeps_the_adopted_group() {
    let mut machine = FakeMachine::new("setup-rdp-rerun-fails");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let before = machine.state.clone();
    machine.fail("deny_remote_logon 1000");
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::RemoteLogon);
    assert!(failure.rolled_back, "{}", failure.cause);
    assert_eq!(machine.state.group, Some(sid(1000)));
    assert_eq!(machine.state.record, before.record);
    assert_eq!(
        denied(&machine.state),
        [sid(1000)],
        "the deny the last run made stays"
    );
    assert!(
        !machine
            .calls
            .iter()
            .any(|c| c.starts_with("remove_remote_logon_deny"))
    );
}

#[test]
fn a_rollback_takes_the_deny_off_a_group_it_made_before_deleting_it() {
    let mut machine = FakeMachine::new("setup-rdp-rollback");
    machine.fail("write_marker");
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert!(failure.rolled_back, "{}", failure.cause);
    assert!(machine.state.remote_denied.is_empty());
    let off = machine
        .calls
        .iter()
        .position(|c| c == "remove_remote_logon_deny 1000")
        .unwrap();
    let deleted = machine
        .calls
        .iter()
        .position(|c| c == "delete_group 1000")
        .unwrap();
    assert!(off < deleted, "{:?}", machine.calls);
}

#[test]
fn a_removal_takes_the_deny_off_before_deleting_the_group() {
    let (mut machine, mut acl) = pool("remove-rdp", Some(owner()));
    assert_eq!(denied(&machine.m.state), [sid(1000)]);
    remove(&mut machine, &mut acl, false).unwrap();
    assert!(machine.m.state.remote_denied.is_empty());
    let lines = machine.lines();
    let at = |line: &str| lines.iter().position(|l| l == line).unwrap();
    assert!(at("group_members 1000") < at("remove_remote_logon_deny 1000"));
    assert!(at("remove_remote_logon_deny 1000") < at("delete_group 1000"));
}

#[test]
fn a_removal_takes_the_deny_off_by_the_records_sid_when_the_group_is_gone() {
    let (mut machine, mut acl) = pool("remove-rdp-gone", Some(owner()));
    // The group deleted outside the removal, its deny left keyed by its SID.
    machine.m.state.group = None;
    assert_eq!(denied(&machine.m.state), [sid(1000)]);
    remove(&mut machine, &mut acl, false).unwrap();
    assert!(machine.m.state.remote_denied.is_empty());
    let lines = machine.lines();
    assert!(
        lines.iter().any(|l| l == "remove_remote_logon_deny 1000"),
        "{lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("delete_group")),
        "{lines:?}"
    );
}

#[test]
fn a_deny_that_wont_come_off_stops_the_removal_with_the_group_kept() {
    let (mut machine, mut acl) = pool("remove-rdp-fails", Some(owner()));
    let at = full_removal()
        .iter()
        .position(|l| l == "remove_remote_logon_deny 1000")
        .unwrap();
    machine.fail_at.set(Some(at));
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (RemoveStep::RemoteLogon, SetupExit::RemovedPartly)
    );
    assert!(failure.cause.contains("injected"), "{}", failure.cause);
    assert!(!failure.cause.contains("S-1-"), "{}", failure.cause);
    assert_eq!(machine.m.state.group, Some(sid(1000)));
    assert_eq!(denied(&machine.m.state), [sid(1000)]);
    assert!(
        machine.m.state.record.is_some(),
        "the record names the group still"
    );
    // The next run takes the deny off, then the group.
    remove(&mut machine, &mut acl, false).unwrap();
    assert!(machine.m.state.remote_denied.is_empty());
    assert_all_gone(&machine, &acl);
}
