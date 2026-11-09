//! The per-machine stop and sweep tasks (`late_steps::stop_task`, `remove::remove_stop_task`): on
//! a per-machine install the set-up registers `\Clauseo\SandboxStop` and `\Clauseo\SandboxSweep`
//! for the owner it records (one `Machine` call, `refresh::POOL_TASKS` on Windows), a new
//! owner's set-up replaces a previous owner's, the removal deletes them before the record, and a
//! failed registration fails the set-up closed. A per-user install's are `--adopt`'s, put back
//! by the set-up when missing or changed (`user_tasks.rs`).

use super::fake::{FakeAcl, FakeMachine, OTHER_OWNER, rid};
use super::remove_fake::Remover;
use super::*;
use crate::windows::setup::record_values::CopyOwner;

/// A machine whose admin copy `copy` placed, with nothing set up yet.
fn installed(test: &str, copy: Option<CopyOwner>) -> FakeMachine {
    let mut machine = FakeMachine::new(test);
    machine.state.copy_owner = copy;
    machine
}

/// The owner the stop task names, as its relative id.
fn task_owner(machine: &FakeMachine) -> Option<String> {
    machine.state.stop_task.as_ref().map(rid)
}

#[test]
fn per_machine_set_up_registers_the_stop_task_for_the_recorded_owner() {
    let mut machine = installed("setup-stop-task-fresh", Some(CopyOwner::Machine));
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    assert_eq!(task_owner(&machine).as_deref(), Some("1001"));
    // Registered once the record names the owner, before anything after the users' steps.
    let calls = machine.calls_since(0);
    let at = calls.iter().position(|c| *c == "register_stop_task 1001");
    let at = at.unwrap_or_else(|| panic!("{calls:?}"));
    let last_write = calls.iter().rposition(|c| c.starts_with("write_record"));
    assert!(last_write.is_some_and(|write| write < at), "{calls:?}");
    let mut expected = fresh_calls();
    expected.insert(at, "register_stop_task 1001".to_owned());
    assert_eq!(calls, expected);
    // A re-run registers it again, over the one there.
    let from = machine.calls.len();
    let req = request(&mut machine, 0, Vec::new());
    set_up(&mut machine, &mut acl, &req).unwrap();
    let again = machine.calls_since(from);
    assert!(again.contains(&"register_stop_task 1001"), "{again:?}");
    assert_eq!(task_owner(&machine).as_deref(), Some("1001"));
}

#[test]
fn a_new_owners_set_up_replaces_the_previous_owners_task() {
    // The previous owner's pool is gone, but their task is still there (its removal failed to
    // delete it): the new owner's set-up names the new owner alone.
    let mut machine = installed("setup-stop-task-owner", Some(CopyOwner::Machine));
    machine.state.stop_task = Some(Sid::parse(OTHER_OWNER).unwrap());
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    assert_eq!(task_owner(&machine).as_deref(), Some("1001"));
    let record = machine.state.record.as_ref().unwrap();
    assert_eq!(machine.state.stop_task.as_ref(), Some(&record.owner));
}

#[test]
fn a_failed_registration_fails_the_set_up_closed() {
    let mut machine = installed("setup-stop-task-fail", Some(CopyOwner::Machine));
    let before = machine.state.clone();
    machine.fail("register_stop_task 1001");
    let mut acl = FakeAcl::default();
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::StopTask);
    assert_eq!(failure.exit, SetupExit::Failed);
    assert!(failure.rolled_back, "{failure:?}");
    assert!(failure.cause.contains("SandboxStop"), "{}", failure.cause);
    assert!(failure.cause.contains("injected"), "{}", failure.cause);
    // Everything the run made is gone again: no record, no pool user, no task.
    assert_eq!(machine.state, before);
    assert!(acl.aces.is_empty(), "{:?}", acl.aces);
}

#[test]
fn per_user_and_copyless_set_ups_register_no_per_machine_task() {
    let mut machine = installed("setup-stop-task-none", None);
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(machine.calls, fresh_calls());
    assert_eq!(machine.state.stop_task, None);
    // Per user, the set-up puts back `--adopt`'s tasks instead, here both missing
    // (`user_tasks.rs`), where the per-machine one registers its own.
    let mut machine = installed("setup-stop-task-user", Some(CopyOwner::User(owner())));
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(machine.state.stop_task, None);
    let calls = machine.calls_since(0);
    let at = calls.iter().position(|c| *c == "task_exe");
    let at = at.unwrap_or_else(|| panic!("{calls:?}"));
    let owner = rid(&owner());
    let mut expected = fresh_calls();
    let ensured = [
        "task_exe".to_owned(),
        format!("replace_task SandboxStop {owner}"),
        format!("replace_task SandboxSweep {owner}"),
    ];
    expected.splice(at..at, ensured);
    assert_eq!(calls, expected);
}

/// A pool set up on a machine whose admin copy `copy` placed, with the stop task as the
/// install left it, for a per-machine removal by its owner.
fn pool(test: &str, copy: Option<CopyOwner>) -> (Remover, super::remove_fake::RemoveAcl) {
    let mut machine = installed(test, copy);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    // Per user, `--adopt` registered it; the fake set-up didn't.
    machine.state.stop_task = Some(owner());
    let remover = Remover::new(machine, Some(owner()));
    let acl = remover.acl(acl);
    (remover, acl)
}

#[test]
fn removing_a_per_machine_pool_deletes_the_stop_task_before_the_record() {
    let (mut machine, mut acl) = pool("remove-stop-task", Some(CopyOwner::Machine));
    remove(&mut machine, &mut acl, true).unwrap();
    let lines = machine.lines();
    let at = lines.iter().position(|l| l == "unregister_stop_task");
    let at = at.unwrap_or_else(|| panic!("{lines:?}"));
    assert_eq!(lines[at + 1], "delete_record");
    assert_eq!(machine.m.state.stop_task, None);
    assert_eq!(machine.m.state.record, None);
    // A deletion that fails stops the removal there, the record kept for the next run.
    let (mut machine, mut acl) = pool("remove-stop-task-fail", Some(CopyOwner::Machine));
    machine.fail_at.set(Some(at));
    let failure = remove(&mut machine, &mut acl, true).unwrap_err();
    assert_eq!(failure.step, RemoveStep::StopTask);
    assert_eq!(failure.exit, SetupExit::RemovedPartly);
    assert!(machine.m.state.record.as_ref().is_some_and(|r| r.removing));
    assert!(machine.m.state.stop_task.is_some());
    remove(&mut machine, &mut acl, true).unwrap();
    assert_eq!(machine.m.state.stop_task, None);
}

#[test]
fn removing_a_per_user_pool_leaves_adopts_task() {
    let (mut machine, mut acl) = pool("remove-stop-task-user", Some(CopyOwner::User(owner())));
    remove(&mut machine, &mut acl, false).unwrap();
    assert!(!machine.lines().iter().any(|l| l == "unregister_stop_task"));
    assert_eq!(machine.m.state.stop_task.as_ref(), Some(&owner()));
}
