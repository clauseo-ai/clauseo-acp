//! The set-up locks the admin copy again once it has recorded the group
//! (`late_steps::admin_lock`): the installer's first `--adopt` locked it before there was a
//! group, so only this lock denies the pool's group. A machine with no admin copy has nothing
//! to lock, and a lock that fails fails the set-up closed.

use super::fake::{FakeAcl, FakeMachine, rid};
use super::*;
use crate::windows::setup::record_values::CopyOwner;

/// A machine whose admin copy `copy` placed, with nothing set up yet.
fn installed(test: &str, copy: Option<CopyOwner>) -> FakeMachine {
    let mut machine = FakeMachine::new(test);
    machine.state.copy_owner = copy;
    machine
}

/// The calls that lock the admin copy.
fn locks(calls: &[&str]) -> Vec<String> {
    calls
        .iter()
        .filter(|call| call.starts_with("lock_admin_copy"))
        .map(|call| (*call).to_owned())
        .collect()
}

#[test]
fn set_up_locks_the_admin_copy_with_the_group_right_after_recording_it() {
    for copy in [Some(CopyOwner::User(owner())), Some(CopyOwner::Machine)] {
        let mut machine = installed("setup-admin-lock", copy.clone());
        let mut acl = FakeAcl::default();
        set_up_fresh(&mut machine, &mut acl);
        let group = machine
            .state
            .record
            .as_ref()
            .map(|r| rid(&r.group))
            .unwrap();
        let line = format!("lock_admin_copy {group}");
        let calls = machine.calls_since(0);
        assert_eq!(locks(&calls), std::slice::from_ref(&line), "{copy:?}");
        // After the group is made and the record's last write names it, before anything else.
        let at = calls.iter().position(|c| *c == line).unwrap();
        let made = calls.iter().position(|c| *c == "create_group").unwrap();
        let last_write = calls.iter().rposition(|c| c.starts_with("write_record"));
        assert!(made < at && last_write.is_some_and(|w| w < at), "{calls:?}");
        let mut expected = fresh_calls();
        if copy == Some(CopyOwner::Machine) {
            expected.insert(at, "register_stop_task 1001".to_owned());
        }
        expected.insert(at, line.clone());
        assert_eq!(calls, expected, "{copy:?}");
        // A re-run adopts the recorded group and locks again with it.
        let from = machine.calls.len();
        let req = request(&mut machine, 0, Vec::new());
        set_up(&mut machine, &mut acl, &req).unwrap();
        assert_eq!(locks(&machine.calls_since(from)), [line], "{copy:?}");
    }
}

#[test]
fn a_machine_with_no_admin_copy_has_nothing_to_lock() {
    let mut machine = installed("setup-admin-lock-none", None);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    assert_eq!(locks(&machine.calls_since(0)), Vec::<String>::new());
    assert_eq!(machine.calls, fresh_calls());
}

#[test]
fn a_lock_that_fails_fails_the_set_up_closed() {
    let mut machine = installed("setup-admin-lock-fail", Some(CopyOwner::Machine));
    let before = machine.state.clone();
    // The group the fake makes first is relative id 1000.
    machine.fail("lock_admin_copy 1000");
    let mut acl = FakeAcl::default();
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::AdminLock);
    assert_eq!(failure.exit, SetupExit::Failed);
    assert!(failure.rolled_back, "{failure:?}");
    assert!(failure.cause.contains("admin copy"), "{}", failure.cause);
    assert!(failure.cause.contains("injected"), "{}", failure.cause);
    assert_eq!(machine.state, before);
}
