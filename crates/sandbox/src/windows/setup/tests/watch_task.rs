//! The watch task (`late_steps::watch_task`, `remove::remove_watch_task`): set-up registers
//! `\Clauseo\SandboxWatch` for the owner it records wherever an admin copy is installed (the
//! installer never does), a failed registration doesn't fail the set-up, and the removal
//! deletes it before the stop task and the record.

use super::fake::{FakeAcl, FakeMachine, OTHER_OWNER, rid};
use super::remove_fake::Remover;
use super::*;
use crate::windows::setup::record_values::CopyOwner;

/// The watch task's removal's lines when no watcher runs and none left its key.
pub(super) const WATCH_TEARDOWN: [&str; 4] = [
    "end_watch_task",
    "stop_watch_session",
    "delete_watch_key",
    "delete_watch_task",
];

/// A machine whose admin copy `copy` placed, with nothing set up yet.
fn installed(test: &str, copy: Option<CopyOwner>) -> FakeMachine {
    let mut machine = FakeMachine::new(test);
    machine.state.copy_owner = copy;
    machine
}

#[test]
fn set_up_registers_the_watch_task_on_any_admin_copy_right_after_the_stop_task() {
    for (test, copy) in [
        ("setup-watch-task-machine", CopyOwner::Machine),
        ("setup-watch-task-user", CopyOwner::User(owner())),
    ] {
        let per_machine = copy == CopyOwner::Machine;
        let mut machine = installed(test, Some(copy));
        let mut acl = FakeAcl::default();
        set_up_fresh(&mut machine, &mut acl);
        assert_eq!(
            machine.state.watch_task.as_ref().map(rid).as_deref(),
            Some("1001")
        );
        let calls = machine.calls_since(0);
        let at = calls.iter().position(|c| *c == "register_watch_task 1001");
        let at = at.unwrap_or_else(|| panic!("{test}: {calls:?}"));
        let last_write = calls.iter().rposition(|c| c.starts_with("write_record"));
        assert!(
            last_write.is_some_and(|write| write < at),
            "{test}: {calls:?}"
        );
        if per_machine {
            assert_eq!(calls[at - 1], "register_stop_task 1001", "{test}");
        }
        // Once each run, and a re-run registers it again.
        assert_eq!(calls.iter().filter(|c| c.contains("watch")).count(), 1);
        let from = machine.calls.len();
        let req = request(&mut machine, 0, Vec::new());
        set_up(&mut machine, &mut acl, &req).unwrap();
        let again = machine.calls_since(from);
        assert!(again.contains(&"register_watch_task 1001"), "{again:?}");
    }
}

#[test]
fn a_new_owners_set_up_replaces_the_previous_owners_watch_task() {
    let mut machine = installed("setup-watch-task-owner", Some(CopyOwner::Machine));
    machine.state.watch_task = Some(Sid::parse(OTHER_OWNER).unwrap());
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let record = machine.state.record.as_ref().unwrap();
    assert_eq!(machine.state.watch_task.as_ref(), Some(&record.owner));
}

#[test]
fn a_copyless_set_up_registers_no_watch_task() {
    let mut machine = installed("setup-watch-task-none", None);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    assert_eq!(machine.calls, fresh_calls());
    assert_eq!(machine.state.watch_task, None);
}

#[test]
fn a_failed_watch_registration_still_sets_up() {
    let mut machine = installed("setup-watch-task-fail", Some(CopyOwner::Machine));
    machine.fail("register_watch_task 1001");
    let mut acl = FakeAcl::default();
    let req = request(&mut machine, 0, Vec::new());
    set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(machine.state.watch_task, None);
    assert!(machine.state.record.is_some());
    assert_eq!(
        machine.state.stop_task.as_ref().map(rid).as_deref(),
        Some("1001")
    );
}

/// A pool set up on a machine whose admin copy `copy` placed, for a removal by its owner.
pub(super) fn pool(
    test: &str,
    copy: Option<CopyOwner>,
) -> (Remover, super::remove_fake::RemoveAcl) {
    let mut machine = installed(test, copy);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let remover = Remover::new(machine, Some(owner()));
    let acl = remover.acl(acl);
    (remover, acl)
}

#[test]
fn the_removal_deletes_the_watch_task_before_the_stop_task_and_the_record() {
    let (mut machine, mut acl) = pool("remove-watch-task", Some(CopyOwner::Machine));
    assert!(machine.m.state.watch_task.is_some());
    remove(&mut machine, &mut acl, true).unwrap();
    let lines = machine.lines();
    let at = lines.iter().position(|l| l == "unregister_watch_task");
    let at = at.unwrap_or_else(|| panic!("{lines:?}"));
    // A watcher that ended on its own left no session and no key (`watch_remove.rs`).
    assert_eq!(lines[at + 1..at + 5], WATCH_TEARDOWN);
    assert_eq!(lines[at + 5], "unregister_stop_task");
    assert_eq!(lines[at + 6], "delete_record");
    assert_eq!(machine.m.state.watch_task, None);
    // A deletion that fails stops the removal there, the record kept for the next run.
    let (mut machine, mut acl) = pool("remove-watch-task-fail", Some(CopyOwner::Machine));
    machine.fail_at.set(Some(at));
    let failure = remove(&mut machine, &mut acl, true).unwrap_err();
    assert_eq!(failure.step, RemoveStep::WatchTask);
    assert_eq!(failure.exit, SetupExit::RemovedPartly);
    assert!(machine.m.state.record.as_ref().is_some_and(|r| r.removing));
    assert!(machine.m.state.watch_task.is_some());
    remove(&mut machine, &mut acl, true).unwrap();
    assert_eq!(machine.m.state.watch_task, None);
}

#[test]
fn a_per_user_removal_deletes_the_watch_task_too_and_a_copyless_one_doesnt_ask() {
    let (mut machine, mut acl) = pool("remove-watch-task-user", Some(CopyOwner::User(owner())));
    remove(&mut machine, &mut acl, false).unwrap();
    assert!(machine.lines().iter().any(|l| l == "unregister_watch_task"));
    assert_eq!(machine.m.state.watch_task, None);
    let (mut machine, mut acl) = pool("remove-watch-task-none", None);
    remove(&mut machine, &mut acl, false).unwrap();
    assert!(!machine.lines().iter().any(|l| l == "unregister_watch_task"));
}
