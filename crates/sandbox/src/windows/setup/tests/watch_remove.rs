//! The watch task's removal (`watch::teardown`) inside `--remove`: a watcher that was killed,
//! or still runs, leaves its session and a key SYSTEM owns, which the removal takes and deletes
//! before the task; a run that fails partway never leaves the task, and the next one finishes.

use super::fake_more::{KeyOwner, Watcher};
use super::remove_fake::{RemoveAcl, Remover};
use super::watch_task::{WATCH_TEARDOWN, pool};
use super::*;
use crate::windows::setup::record_values::CopyOwner;

/// A pool on a per-machine install whose watcher is in `watcher`'s state.
fn with_watcher(test: &str, watcher: &Watcher) -> (Remover, RemoveAcl) {
    let (mut machine, acl) = pool(test, Some(CopyOwner::Machine));
    assert!(machine.m.state.watch_task.is_some());
    machine.m.state.watcher = watcher.clone();
    (machine, acl)
}

/// The lines from the watch task's removal to the record's delete.
fn watch_lines(machine: &Remover) -> Vec<String> {
    let lines = machine.lines();
    let from = lines.iter().position(|l| l == "unregister_watch_task");
    let to = lines.iter().position(|l| l == "delete_record");
    lines[from.unwrap() + 1..to.unwrap()].to_vec()
}

/// Everything the removal takes is gone: the watcher's parts, both tasks and the record.
fn assert_all_gone(machine: &Remover) {
    let state = &machine.m.state;
    assert_eq!(state.watch_task, None);
    assert_eq!(state.watcher, Watcher::default());
    assert_eq!(state.stop_task, None);
    assert_eq!(state.record, None);
}

#[test]
fn a_killed_watchers_system_key_is_taken_and_deleted_before_the_task_and_the_removal_ends_0() {
    for (test, watcher) in [
        ("remove-watch-killed", Watcher::killed()),
        ("remove-watch-running", Watcher::running()),
    ] {
        let (mut machine, mut acl) = with_watcher(test, &watcher);
        let removed = remove(&mut machine, &mut acl, true);
        assert!(
            matches!(removed, Ok(Removed::All(_))),
            "{test}: {removed:?}"
        );
        assert_eq!(
            watch_lines(&machine),
            [
                "end_watch_task",
                "stop_watch_session",
                "inspect_watch_key",
                "delete_watch_key",
                "take_watch_key",
                "delete_watch_key",
                "delete_watch_task",
                "unregister_stop_task",
            ],
            "{test}"
        );
        assert_all_gone(&machine);
    }
}

#[test]
fn a_watcher_that_stopped_cleanly_is_removed_as_before_with_nothing_taken() {
    let (mut machine, mut acl) = with_watcher("remove-watch-clean", &Watcher::default());
    remove(&mut machine, &mut acl, true).unwrap();
    let lines = watch_lines(&machine);
    assert_eq!(lines[..4], WATCH_TEARDOWN);
    assert_eq!(lines[4..], ["unregister_stop_task"]);
    assert_all_gone(&machine);
}

/// Where `line` falls in a full removal of a killed watcher's pool.
fn place_of(line: &str) -> usize {
    let (mut machine, mut acl) = with_watcher("remove-watch-place", &Watcher::killed());
    remove(&mut machine, &mut acl, true).unwrap();
    let lines = machine.lines();
    lines.iter().position(|l| l == line).unwrap()
}

#[test]
fn a_take_that_fails_still_deletes_the_task_and_a_second_removal_finishes_the_key() {
    let at = place_of("take_watch_key");
    let (mut machine, mut acl) = with_watcher("remove-watch-take-fails", &Watcher::killed());
    machine.fail_at.set(Some(at));
    let failure = remove(&mut machine, &mut acl, true).unwrap_err();
    assert_eq!(failure.step, RemoveStep::WatchTask);
    assert_eq!(failure.exit, SetupExit::RemovedPartly);
    // The task never outlives the failure; the key does, and the record is kept for it.
    let state = &machine.m.state;
    assert_eq!(state.watch_task, None);
    assert_eq!(state.watcher.key, Some(KeyOwner::System));
    assert!(!state.watcher.session);
    assert!(state.record.as_ref().is_some_and(|r| r.removing));
    // A key without its task: the second run takes and deletes it, and ends 0.
    let from = machine.lines().len();
    let removed = remove(&mut machine, &mut acl, true);
    assert!(matches!(removed, Ok(Removed::All(_))), "{removed:?}");
    let again = machine.lines()[from..].to_vec();
    let take = again.iter().position(|l| l == "take_watch_key").unwrap();
    assert_eq!(
        again[take + 1..take + 3],
        ["delete_watch_key", "delete_watch_task"]
    );
    assert_all_gone(&machine);
}

#[test]
fn a_task_delete_that_fails_leaves_no_key_and_a_second_removal_finishes_the_task() {
    let at = place_of("delete_watch_task");
    let (mut machine, mut acl) = with_watcher("remove-watch-task-fails", &Watcher::killed());
    machine.fail_at.set(Some(at));
    let failure = remove(&mut machine, &mut acl, true).unwrap_err();
    assert_eq!(failure.step, RemoveStep::WatchTask);
    assert_eq!(failure.exit, SetupExit::RemovedPartly);
    assert!(machine.m.state.watch_task.is_some());
    assert_eq!(machine.m.state.watcher, Watcher::default());
    // A task without its key: nothing is taken, and the task goes.
    let from = machine.lines().len();
    remove(&mut machine, &mut acl, true).unwrap();
    assert!(
        !machine.lines()[from..]
            .iter()
            .any(|l| l == "take_watch_key")
    );
    assert_all_gone(&machine);
}

#[test]
fn a_link_or_a_key_not_systems_is_never_taken_or_deleted_and_the_tasks_still_go() {
    for (test, key) in [
        ("remove-watch-link", KeyOwner::Link),
        ("remove-watch-other", KeyOwner::Administrators),
        ("remove-watch-unreadable", KeyOwner::Unreadable),
    ] {
        let watcher = Watcher {
            key: Some(key),
            ..Watcher::killed()
        };
        let (mut machine, mut acl) = with_watcher(test, &watcher);
        for run in 0..2 {
            let failure = remove(&mut machine, &mut acl, true).unwrap_err();
            assert_eq!(failure.step, RemoveStep::WatchTask, "{test} {run}");
            assert_eq!(failure.exit, SetupExit::RemovedPartly, "{test} {run}");
            let state = &machine.m.state;
            assert_eq!(state.watch_task, None, "{test} {run}");
            assert_eq!(state.watcher.key, Some(key), "{test} {run}");
            assert!(!state.watcher.session, "{test} {run}");
            assert!(state.record.is_some(), "{test} {run}");
        }
        let lines = machine.lines();
        let touched = lines
            .iter()
            .any(|l| l == "take_watch_key" || l == "delete_watch_key");
        assert!(!touched, "{test}: {lines:?}");
        assert!(lines.iter().any(|l| l == "delete_watch_task"), "{test}");
    }
}
