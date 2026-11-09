//! Set-up's COM launch deny (step 8c): after the network, as admin, once the group and the
//! record exist; a degraded guard (Group Policy, an unparsable value) or a failed step leaves
//! the set-up done and is reported; the rollback takes the deny off before the group it made
//! goes, and keeps a group it can't take the deny off; `--remove` takes it off before the
//! group.

use super::*;
use crate::windows::machine_launch::{Change, Guard, LaunchApplied, LaunchRemoved};
use crate::windows::setup::remove::RemoveStep;

#[test]
fn the_deny_follows_the_network_as_admin_and_is_reported() {
    let mut machine = FakeMachine::new("launch-deny-order");
    let mut acl = FakeAcl::default();
    let report = set_up_fresh(&mut machine, &mut acl);
    let applied = report.launch_deny.unwrap();
    assert_eq!(
        (applied.guard, applied.change),
        (Guard::Applied, Change::Added)
    );
    let at = |line: &str| {
        machine
            .calls
            .iter()
            .position(|c| c.starts_with(line))
            .unwrap()
    };
    assert!(at("write_record") < at("launch_deny 1000"));
    assert!(at("network") < at("launch_deny 1000"));
    assert!(at("launch_deny 1000") < at("write_marker"));
}

#[test]
fn a_degraded_guard_or_a_failed_step_leaves_the_set_up_done() {
    for guard in [Guard::PolicyOverrides, Guard::Unparsable, Guard::Failed] {
        let mut machine = FakeMachine::new("launch-deny-degraded");
        machine.launch = Some(LaunchApplied {
            guard,
            change: Change::Unchanged,
            why: Some("a Group Policy value overrides it".to_owned()),
        });
        let mut acl = FakeAcl::default();
        let report = set_up_fresh(&mut machine, &mut acl);
        assert_eq!(report.launch_deny.map(|done| done.guard), Ok(guard));
        assert!(machine.state.marker.is_some(), "{guard:?}");
    }
    let mut machine = FakeMachine::new("launch-deny-failed");
    machine.fail("launch_deny 1000");
    let mut acl = FakeAcl::default();
    let req = request(&mut machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert!(report.launch_deny.is_err(), "{:?}", report.launch_deny);
    assert!(machine.state.marker.is_some());
}

#[test]
fn a_rollback_keeps_a_group_whose_deny_wont_come_off() {
    let mut machine = FakeMachine::new("launch-deny-rollback");
    machine.fail("write_marker");
    machine.fail("launch_deny_remove 1000");
    let mut acl = FakeAcl::default();
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    // Left behind, so not a full rollback: the group and the record naming it.
    assert!(!failure.rolled_back, "{failure:?}");
    assert!(!machine.calls.iter().any(|c| c == "delete_group 1000"));
    assert_eq!(
        machine.written.last().map(|r| r.group.clone()),
        machine.state.group.clone()
    );
    assert!(machine.state.group.is_some());
}

#[test]
fn removal_takes_the_deny_off_before_the_group() {
    let calls = super::remove::full_removal();
    let at = |line: &str| calls.iter().position(|c| c == line).unwrap();
    assert!(at("walk ProgramData/Microsoft/DeviceSync") < at("remove_launch_deny 1000"));
    assert!(at("remove_launch_deny 1000") < at("delete_group 1000"));
    assert_eq!(
        RemoveStep::LaunchDeny.to_string(),
        "taking the sandbox users' COM launch deny off"
    );
    assert_eq!(LaunchRemoved::default(), LaunchRemoved::Nothing);
}
