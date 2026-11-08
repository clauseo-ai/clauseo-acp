//! The SYSTEM tasks (`late_steps::tasks`, `remove::remove_stop_task`): with the owner's own
//! per-user admin copy, the set-up registers `\Clauseo\SandboxRefresh` then
//! `\Clauseo\SandboxStop` for the owner it records, with a per-machine one the stop task
//! alone, then refuses the pool's group the set-up task, each again on a re-run; a new owner's
//! set-up replaces a previous owner's; a failed registration fails the set-up closed; another
//! user's copy, or none, gets nothing. A per-machine removal deletes the stop task before the
//! record.

use super::fake::{FakeAcl, FakeMachine, OTHER_OWNER, Run, rid};
use super::remove_fake::Remover;
use super::*;
use crate::windows::setup::record_values::CopyOwner;

/// A machine whose admin copy `copy` placed, with nothing set up yet.
fn installed(test: &str, copy: Option<CopyOwner>) -> FakeMachine {
    let mut machine = FakeMachine::new(test);
    machine.state.copy_owner = copy;
    machine
}

/// The owners the refresh and stop tasks name, as relative ids.
fn task_owners(machine: &FakeMachine) -> (Option<String>, Option<String>) {
    let state = &machine.state;
    (
        state.refresh_task.as_ref().map(rid),
        state.stop_task.as_ref().map(rid),
    )
}

/// The task owners registered for 1001: the refresh task's when `refresh`, and the stop task's.
fn for_owner(refresh: bool) -> (Option<String>, Option<String>) {
    let owner = Some("1001".to_owned());
    (owner.clone().filter(|_| refresh), owner)
}

/// The registrations per user, in order; per machine, the stop task's (the last) alone.
const REGISTERED: [&str; 2] = ["register_refresh_task 1001", "register_stop_task 1001"];

/// A fresh set-up registers `want` once the record names the owner, right after its write and
/// before anything after the users' steps; a re-run (the elevated first run again, or any later
/// set-up) registers them again over the ones there, changing nothing else about them. Then an
/// elevated set-up (`heals`) registers the set-up task again if missing or changed: per user for
/// the owner, per machine for interactive users with the group refused. The set-up task's own
/// run doesn't; per machine it refuses the recorded group the set-up task instead.
fn registers_then_again(test: &str, copy: CopyOwner, want: &[&str], heals: bool) {
    let refresh = want.len() == 2;
    let mut machine = installed(test, Some(copy));
    machine.run = if heals { Run::Elevated } else { Run::Task };
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let group = machine.state.group.clone().unwrap();
    let mut want: Vec<&str> = want.to_vec();
    let (task, denied) = match (refresh, heals) {
        (true, true) => {
            want.push("ensure_set_up_task owner 1001");
            (Some((owner(), None)), None)
        }
        (false, true) => {
            want.push("ensure_set_up_task interactive");
            (Some((owner(), Some(group))), None)
        }
        (true, false) => (None, None),
        (false, false) => {
            want.push("deny_set_up_to");
            (None, Some(group))
        }
    };
    assert_eq!(machine.state.set_up_task, task, "{test}");
    assert_eq!(machine.state.set_up_denied, denied, "{test}");
    let want = want.as_slice();
    assert_eq!(task_owners(&machine), for_owner(refresh), "{test}");
    let calls = machine.calls_since(0);
    let at = calls.iter().position(|c| *c == want[0]);
    let at = at.unwrap_or_else(|| panic!("{calls:?}"));
    assert_eq!(calls[at..at + want.len()], *want, "{calls:?}");
    let last_write = calls.iter().rposition(|c| c.starts_with("write_record"));
    assert!(last_write.is_some_and(|write| write < at), "{calls:?}");
    let mut expected = fresh_calls();
    for call in want.iter().rev() {
        expected.insert(at, (*call).to_owned());
    }
    assert_eq!(calls, expected, "{test}");
    for _ in 0..2 {
        let state = machine.state.clone();
        let from = machine.calls.len();
        let req = request(&mut machine, 0, Vec::new());
        set_up(&mut machine, &mut acl, &req).unwrap();
        let again = machine.calls_since(from);
        let found: Vec<&str> = again
            .iter()
            .copied()
            .filter(|c| {
                c.starts_with("register_") && c.ends_with("_task 1001")
                    || c.starts_with("ensure_set_up_task")
                    || *c == "deny_set_up_to"
            })
            .collect();
        assert_eq!(found, want, "{test}: {again:?}");
        assert_eq!(task_owners(&machine), for_owner(refresh), "{test}");
        assert_eq!(machine.state.copy_owner, state.copy_owner, "{test}");
        assert_eq!(machine.state.record, state.record, "{test}");
    }
}

#[test]
fn per_machine_set_up_registers_the_stop_task_for_the_recorded_owner_idempotently() {
    // No refresh task: IT updates a per-machine install.
    registers_then_again(
        "setup-tasks-machine",
        CopyOwner::Machine,
        &REGISTERED[1..],
        true,
    );
    // The set-up task's own run: the deny alone, no registration of itself.
    registers_then_again(
        "setup-tasks-machine-task",
        CopyOwner::Machine,
        &REGISTERED[1..],
        false,
    );
}

#[test]
fn per_user_set_up_registers_both_tasks_for_its_copy_owner_idempotently() {
    // `--adopt --place-only` placed the copy and registered nothing; the app's first run's
    // set-up, from the admin copy, makes both tasks.
    registers_then_again(
        "setup-tasks-user",
        CopyOwner::User(owner()),
        &REGISTERED,
        true,
    );
    registers_then_again(
        "setup-tasks-user-task",
        CopyOwner::User(owner()),
        &REGISTERED,
        false,
    );
}

#[test]
fn a_new_owners_set_up_replaces_the_previous_owners_task() {
    // The previous owner's pool is gone, but their task is still there (its removal failed to
    // delete it): the new owner's set-up names the new owner alone.
    let mut machine = installed("setup-stop-task-owner", Some(CopyOwner::Machine));
    machine.state.stop_task = Some(Sid::parse(OTHER_OWNER).unwrap());
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    assert_eq!(task_owners(&machine), for_owner(false));
    let record = machine.state.record.as_ref().unwrap();
    assert_eq!(machine.state.stop_task.as_ref(), Some(&record.owner));
}

#[test]
fn a_failed_registration_fails_the_set_up_closed() {
    let user = || CopyOwner::User(owner());
    let stop = (Step::StopTask, "SandboxStop");
    let refresh = (Step::RefreshTask, "SandboxRefresh");
    let set_up_task = (Step::SetUpTask, "SandboxSetUp");
    for (test, copy, failing, (step, name)) in [
        ("setup-tasks-fail-machine", CopyOwner::Machine, 1, stop),
        ("setup-tasks-fail-stop", user(), 1, stop),
        ("setup-tasks-fail-refresh", user(), 0, refresh),
        ("setup-tasks-fail-deny", CopyOwner::Machine, 2, set_up_task),
        ("setup-tasks-fail-heal", CopyOwner::Machine, 3, set_up_task),
    ] {
        let per_user = copy != CopyOwner::Machine;
        let mut machine = installed(test, Some(copy));
        let mut before = machine.state.clone();
        match failing {
            2 => {
                machine.run = Run::Task;
                machine.fail("deny_set_up_to");
            }
            3 => machine.fail("ensure_set_up_task interactive"),
            _ => machine.fail(REGISTERED[failing]),
        }
        let mut acl = FakeAcl::default();
        let req = request(&mut machine, 0, Vec::new());
        let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
        assert_eq!(failure.step, step, "{test}");
        assert_eq!(failure.exit, SetupExit::Failed, "{test}");
        assert!(failure.rolled_back, "{failure:?}");
        assert!(failure.cause.contains(name), "{}", failure.cause);
        assert!(failure.cause.contains("injected"), "{}", failure.cause);
        // Everything the run made is gone again: no record, no pool user, and no task but a
        // refresh task registered before the stop task failed (the rollback has no step for it).
        if per_user && failing == 1 {
            before.refresh_task = Some(owner());
        }
        if failing >= 2 {
            before.stop_task = Some(owner());
        }
        assert_eq!(machine.state, before, "{test}");
        assert!(acl.aces.is_empty(), "{:?}", acl.aces);
    }
}

#[test]
fn another_users_copy_and_copyless_set_ups_register_nothing() {
    let other = Sid::parse(OTHER_OWNER).unwrap();
    for (test, copy) in [
        ("setup-tasks-other", Some(CopyOwner::User(other))),
        ("setup-tasks-none", None),
    ] {
        let mut machine = installed(test, copy);
        let mut acl = FakeAcl::default();
        set_up_fresh(&mut machine, &mut acl);
        assert_eq!(machine.calls, fresh_calls(), "{test}");
        assert_eq!(task_owners(&machine), (None, None), "{test}");
    }
}

/// A pool set up on a machine whose admin copy `copy` placed, with the stop task as the
/// install left it, for a per-machine removal by its owner.
fn pool(test: &str, copy: Option<CopyOwner>) -> (Remover, super::remove_fake::RemoveAcl) {
    let mut machine = installed(test, copy);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    // The set-up registered it with an admin copy; without one, as if it had.
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
