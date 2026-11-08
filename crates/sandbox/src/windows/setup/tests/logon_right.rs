//! Step 3a, "Allow log on locally" for the group (`logon_grant.rs`), through the engine: the
//! grant where no policy defines the right, the block where a policy leaves the pool out, the
//! rollback of a grant this run made, and `--remove` taking off only what set-up added.

use super::fake::{FakeAcl, FakeMachine};
use super::remove_fake::Remover;
use super::*;
use crate::Blocked;
use crate::windows::setup::remove::{Removed, remove};
use crate::windows::setup::rights_policy::RightPolicy;

/// A machine whose local policy gives the right to Administrators only.
fn admins_only(test: &str) -> FakeMachine {
    let mut machine = FakeMachine::new(test);
    machine.rights.holders = vec![Sid::parse("S-1-5-32-544").unwrap()];
    machine
}

#[test]
fn a_default_machine_needs_no_grant() {
    let mut machine = FakeMachine::new("logon-default");
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(machine.rights.recorded, None);
    assert!(
        !machine
            .rights
            .calls
            .iter()
            .any(|call| call.starts_with("add"))
    );
}

#[test]
fn no_policy_and_no_holder_grants_the_group_and_records_it() {
    let mut machine = admins_only("logon-grant");
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    let group = sid(1000);
    assert_eq!(machine.rights.recorded, Some(group.clone()));
    assert!(machine.rights.holders.contains(&group));
    // Granted before the first pool user logs on.
    let calls = &machine.calls;
    let logon = calls
        .iter()
        .position(|c| c.starts_with("log_on_once"))
        .unwrap();
    let made = calls.iter().position(|c| c == "create_group").unwrap();
    assert!(made < logon, "{calls:?}");
}

#[test]
fn a_policy_without_the_pool_blocks_and_grants_nothing() {
    let mut machine = admins_only("logon-policy");
    machine.rights.policy = RightPolicy::Defined {
        includes_pool: false,
    };
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::LogonRight);
    assert_eq!(failure.cause, LOGON_POLICY_EXCLUDES);
    assert!(
        failure
            .cause
            .starts_with("This computer's settings don't let Clauseo's sandbox")
    );
    assert!(failure.cause.contains("ClauseoSandboxUsers"));
    assert_eq!(failure.blocked(), Some(Blocked::LogonPolicy));
    assert!(failure.rolled_back);
    assert_eq!(machine.state.group, None, "the group this run made is gone");
    assert!(
        !machine
            .calls
            .iter()
            .any(|call| call.starts_with("create_user"))
    );
    assert_eq!(machine.rights.recorded, None);
    assert!(!machine.rights.holders.contains(&sid(1000)));
}

#[test]
fn a_policy_that_cant_be_read_grants_nothing_and_the_logon_decides() {
    let mut machine = admins_only("logon-unknown");
    machine.rights.policy = RightPolicy::Unknown;
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(machine.rights.recorded, None);
    assert!(
        !machine
            .rights
            .calls
            .iter()
            .any(|call| call.starts_with("holders"))
    );
}

#[test]
fn a_failed_run_takes_its_grant_back() {
    let mut machine = admins_only("logon-rollback");
    machine.fails.push((
        "log_on_once ClauseoSb01".to_owned(),
        MachineError::Other("no".to_owned()),
    ));
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert!(failure.rolled_back, "{failure:?}");
    assert_eq!(machine.rights.recorded, None);
    assert_eq!(
        machine.rights.holders,
        [Sid::parse("S-1-5-32-544").unwrap()]
    );
}

#[test]
fn a_fresh_run_records_its_group_before_it_grants() {
    let mut machine = admins_only("logon-record-first");
    // The grant fails: the record was written first and named the group.
    machine.rights.fail = Some(format!("add {}", sid(1000)));
    let req = request(&mut machine, 0, Vec::new());
    set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(
        machine.written.first().map(|r| (&r.group, r.pool.len())),
        Some((&sid(1000), 0))
    );
    assert!(
        !machine
            .calls
            .iter()
            .any(|call| call.starts_with("create_user"))
    );
}

#[test]
fn a_grant_the_rollback_cant_take_off_keeps_the_record() {
    let mut machine = admins_only("logon-undo-fails");
    machine.fails.push((
        "log_on_once ClauseoSb01".to_owned(),
        MachineError::Other("no".to_owned()),
    ));
    machine.rights.fail = Some(format!("remove {}", sid(1000)));
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert!(!failure.rolled_back);
    assert!(
        failure.cause.contains("Allow log on locally"),
        "{}",
        failure.cause
    );
    // The record still names the group, so `--remove` finds the grant and takes it off.
    assert_eq!(
        machine.state.record.as_ref().map(|r| &r.group),
        Some(&sid(1000))
    );
    assert_eq!(machine.rights.recorded, Some(sid(1000)));
    let mut remover = Remover::new(machine, Some(owner()));
    let mut acl = remover.acl(FakeAcl::default());
    remove(&mut remover, &mut acl, false).unwrap();
    assert_eq!(remover.m.rights.recorded, None);
    assert!(!remover.m.rights.holders.contains(&sid(1000)));
}

#[test]
fn remove_takes_off_the_grant_set_up_added() {
    let mut machine = admins_only("logon-remove");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let mut remover = Remover::new(machine, Some(owner()));
    let mut acl = remover.acl(acl);
    let removed = remove(&mut remover, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    assert_eq!(remover.m.rights.recorded, None);
    assert_eq!(
        remover.m.rights.holders,
        [Sid::parse("S-1-5-32-544").unwrap()]
    );
}

#[test]
fn remove_leaves_a_right_set_up_didnt_add() {
    let mut machine = FakeMachine::new("logon-remove-theirs");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    // IT gave the group the right itself, after set-up.
    machine.rights.holders.push(sid(1000));
    let mut remover = Remover::new(machine, Some(owner()));
    let mut acl = remover.acl(acl);
    remove(&mut remover, &mut acl, false).unwrap();
    assert!(remover.m.rights.holders.contains(&sid(1000)));
    assert!(
        !remover
            .m
            .rights
            .calls
            .iter()
            .any(|call| call.starts_with("remove"))
    );
}
