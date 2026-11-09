//! F4 when a marker's file won't change: a clean one that can't be removed refuses nothing,
//! and one whose sweep before its final stop completed but can't be re-marked refuses with no
//! stop requested. And a sweep that can't finish because its task is missing: the refusal says
//! so, in the readiness check's words. And the launch's check of the tasks before the logon:
//! missing or changed refuses it, and tasks that can't be read are read once more first.

use super::*;
use crate::windows::refresh::{TASK_CHANGED, TASK_MISSING, TASK_UNCHECKED};

const SWEPT: MarkerState = MarkerState::AwaitingSweep { sweep: 7 };

/// One run of the refresh task: whether it swaps a new admin copy in, and what it leaves the
/// tasks needing.
pub(super) struct HealRun {
    pub(super) swaps: bool,
    pub(super) leaves: Option<&'static str>,
}

#[test]
fn a_clean_marker_that_cant_be_removed_lets_the_logon_go_without_a_stop() {
    // Its sweep had completed: another program holds the file open.
    let mut met = Fake::new();
    met.marker("a1", Some(POOL_A), Some(SWEPT));
    met.done.sweep = 7;
    met.fails.remove = true;
    assert_eq!(logon_check(&mut met, &sid(POOL_A)), Ok(()));
    assert!(met.has("a1"), "still on disk");
    let note = "note an earlier session's marker a1 is clean but couldn't be removed";
    assert_eq!(met.log, ["can't remove a1", note]);
    // Its sweep completes during the wait.
    let mut waited = Fake::new();
    waited.marker("a1", Some(POOL_A), Some(SWEPT));
    waited.done.sweep = 6;
    waited.sweeps = vec![7];
    waited.fails.remove = true;
    assert_eq!(logon_check(&mut waited, &sid(POOL_A)), Ok(()));
    assert_eq!(waited.asked("stop"), 0);
    assert_eq!(waited.asked(note), 1);
}

#[test]
fn an_unremovable_clean_marker_doesnt_hide_one_that_owes_its_final_stop() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(SWEPT));
    fake.marker("a2", Some(POOL_A), Some(REMARKED));
    fake.done.sweep = 9;
    fake.fails.remove = true;
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop);
    assert!(refused.why.contains("final stop"), "{refused:?}");
    assert_eq!(fake.state("a1"), Some(SWEPT));
    assert_eq!(fake.state("a2"), Some(FINAL));
    assert_eq!(fake.asked("stop"), 1);
}

#[test]
fn a_swept_marker_whose_re_mark_cant_be_written_refuses_the_logon_without_a_stop() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(REMARKED));
    fake.done.sweep = 9;
    fake.fails.settle = true;
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused, Refused::sweep(STUCK_SWEPT.to_owned()));
    assert_eq!(fake.state("a1"), Some(REMARKED));
    assert_eq!(fake.asked("read StopNext"), 1);
    assert_eq!(fake.asked("stop"), 0);
    // Once the write goes through, the re-mark requests the final stop.
    fake.fails.settle = false;
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingStop);
    assert_eq!(fake.state("a1"), Some(FINAL));
}

#[test]
fn a_sweep_refused_while_its_task_is_missing_or_changed_says_to_repair_clauseo() {
    let awaiting = Some(MarkerState::AwaitingSweep { sweep: 6 });
    for words in [TASK_MISSING, TASK_CHANGED, TASK_UNCHECKED] {
        // F4's logon refusal, up front (an unreadable task in the launch's own words).
        let mut fake = Fake::new();
        fake.marker("a1", Some(POOL_A), awaiting);
        fake.task_repair = Some(words);
        let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
        let said = if words == TASK_UNCHECKED {
            TASKS_UNCHECKED_AT_LAUNCH
        } else {
            words
        };
        assert_eq!(refused, Refused::sweep(said.to_owned()));
        assert_eq!(fake.asked("stop"), 0);
        // A launch's refusal says the same.
        assert_eq!(await_sweeps(&mut fake), Err(words.to_owned()));
    }
    assert_eq!(
        TASK_MISSING,
        "Clauseo's clean-up task is missing. Repair Clauseo to put it back."
    );
    assert_eq!(
        TASK_CHANGED,
        "Clauseo's clean-up task has been changed. Repair Clauseo to put it back."
    );
    // With the task as the set-up has it, both still say the sweep didn't finish.
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), awaiting);
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingSweep);
    assert!(
        refused.why.contains("didn't finish within 15 s"),
        "{refused:?}"
    );
    let refused = await_sweeps(&mut fake).unwrap_err();
    assert!(refused.contains("didn't finish within 15 s"), "{refused}");
}

#[test]
fn a_missing_or_changed_task_refuses_the_logon_before_it_happens() {
    // The first launch after the sweep task is deleted: no marker awaits anything yet, and
    // the logon is refused up front rather than at the next launch.
    for words in [TASK_MISSING, TASK_CHANGED] {
        let mut fake = Fake::new();
        fake.task_repair = Some(words);
        let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
        assert_eq!(refused, Refused::sweep(words.to_owned()));
        assert_eq!(refused.exit, RunExit::AwaitingSweep);
        // The refresh task couldn't be started: refused, nothing else asked for or written.
        assert_eq!(fake.log, ["heal"], "nothing requested or written");
        assert_eq!(fake.task_reads, 1);
    }
}

#[test]
fn tasks_that_cant_be_checked_are_read_once_more_then_refuse_the_logon() {
    assert_eq!(TASK_RETRY, Duration::from_millis(300));
    // The first read fails, the retry answers: the launch proceeds.
    let mut fake = Fake::new();
    fake.queued_task_repairs = vec![Some(TASK_UNCHECKED)];
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert_eq!(fake.log, ["pause for the tasks"]);
    assert_eq!(fake.task_reads, 2);
    // The retry finds a task missing: refused in those words.
    let mut fake = Fake::new();
    fake.queued_task_repairs = vec![Some(TASK_UNCHECKED)];
    fake.task_repair = Some(TASK_MISSING);
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused, Refused::sweep(TASK_MISSING.to_owned()));
    // Both fail: refused before the logon, nothing requested or written.
    let mut fake = Fake::new();
    fake.task_repair = Some(TASK_UNCHECKED);
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingSweep);
    assert_eq!(
        refused.why,
        "Clauseo couldn't check its clean-up task, so it didn't start the AI tool. Try again. \
         If this keeps happening, restart your computer."
    );
    assert_eq!(fake.log, ["pause for the tasks"]);
    assert_eq!(fake.task_reads, 2);
}

#[test]
fn a_task_scheduler_that_never_answers_refuses_the_launch_without_hanging() {
    use std::sync::mpsc;

    use crate::windows::refresh::task_call::{Slot, TASK_CALL_LIMIT, repair_within, within};
    static FIRST: Slot<Option<&'static str>> = Slot::new();
    static RETRY: Slot<Option<&'static str>> = Slot::new();
    static ANSWERED: Slot<Option<&'static str>> = Slot::new();
    static STOP: Slot<bool> = Slot::new();
    static SEVEN: Slot<u32> = Slot::new();
    assert_eq!(TASK_CALL_LIMIT, Duration::from_secs(10));
    // Each read hangs; the launch's bounded reads give up (here at 50 ms, at 10 s for real), so
    // this test ends rather than hanging.
    let (keep, never) = mpsc::channel::<()>();
    let (keep_retry, never_again) = mpsc::channel::<()>();
    let hung = |never: mpsc::Receiver<()>| {
        move || {
            let _hung_until_the_test_ends = never.recv();
            None
        }
    };
    let limit = Duration::from_millis(50);
    let mut fake = Fake::new();
    fake.queued_task_repairs = vec![repair_within(&FIRST, limit, hung(never))];
    fake.task_repair = repair_within(&RETRY, limit, hung(never_again));
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(
        refused,
        Refused::sweep(TASKS_UNCHECKED_AT_LAUNCH.to_owned())
    );
    // A request that hangs is given up on too; one that answers is passed on.
    let (keep_stop, stuck) = mpsc::channel::<()>();
    let stop = within(&STOP, limit, move || stuck.recv().is_ok());
    assert!(stop.is_err());
    assert_eq!(within(&SEVEN, Duration::from_secs(5), || 7), Ok(7));
    let answered = repair_within(&ANSWERED, Duration::from_secs(5), || Some(TASK_MISSING));
    assert_eq!(answered, Some(TASK_MISSING));
    drop((keep, keep_retry, keep_stop));
}

#[test]
fn a_missing_task_is_put_back_by_the_refresh_task_before_the_launch_refuses() {
    assert_eq!(
        (HEAL_WAIT, HEAL_POLL),
        (Duration::from_secs(20), Duration::from_millis(500))
    );
    let run = |swaps: bool, leaves: Option<&'static str>| HealRun { swaps, leaves };
    // After an update from the shipped release: no sweep task. The refresh task puts it back,
    // and the launch goes ahead.
    let mut fake = Fake::new();
    fake.task_repair = Some(TASK_MISSING);
    fake.heal_starts = true;
    fake.heal_runs = vec![run(false, None)];
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    assert_eq!(fake.log, ["heal", "pause for the heal"]);
    // The first run is the old copy's and only swaps the new one in; the run asked for once
    // more at once is the new copy's, which registers the sweep task.
    let mut fake = Fake::new();
    fake.task_repair = Some(TASK_MISSING);
    fake.heal_starts = true;
    fake.heal_runs = vec![run(true, Some(TASK_MISSING)), run(false, None)];
    assert_eq!(logon_check(&mut fake, &sid(POOL_A)), Ok(()));
    let heal = ["heal", "pause for the heal"];
    assert_eq!(fake.log, [heal, heal].concat());
    // Neither run registers it: refused after 20 s in all, in the missing words.
    let mut fake = Fake::new();
    fake.task_repair = Some(TASK_MISSING);
    fake.heal_starts = true;
    fake.heal_runs = vec![
        run(true, Some(TASK_MISSING)),
        run(false, Some(TASK_MISSING)),
    ];
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused, Refused::sweep(TASK_MISSING.to_owned()));
    assert_eq!(fake.asked("pause for the heal"), 40);
    // The first ask, once more after the swap, and again every 3 s.
    assert_eq!(fake.asked("heal"), 1 + 1 + 6);
    // A heal that doesn't start: refused in the missing words at once.
    let mut fake = Fake::new();
    fake.task_repair = Some(TASK_MISSING);
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused, Refused::sweep(TASK_MISSING.to_owned()));
    assert_eq!(fake.log, ["heal"]);
    // A Task Scheduler that stops answering while it waits: refused in the changed words.
    let mut fake = Fake::new();
    fake.task_repair = Some(TASK_CHANGED);
    fake.heal_starts = true;
    fake.heal_runs = vec![run(false, Some(TASK_UNCHECKED))];
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused, Refused::sweep(TASK_CHANGED.to_owned()));
    assert_eq!(fake.asked("pause for the heal"), 1);
}

#[test]
fn the_host_is_told_once_while_a_heal_runs_and_never_otherwise() {
    assert_eq!(
        HEALING,
        "Clauseo is finishing an update before it starts the AI tool."
    );
    let run = |swaps: bool, leaves: Option<&'static str>| HealRun { swaps, leaves };
    // A heal that runs, even across the old copy's swap and a second run: told once.
    let mut fake = Fake::new();
    fake.task_repair = Some(TASK_MISSING);
    fake.heal_starts = true;
    fake.heal_runs = vec![run(true, Some(TASK_MISSING)), run(false, None)];
    let mut told = 0;
    assert_eq!(heal_first(&mut fake, || told += 1), None);
    assert_eq!(told, 1);
    // One that fails is told once too, and says what is still missing.
    let mut fake = Fake::new();
    fake.task_repair = Some(TASK_MISSING);
    fake.heal_starts = true;
    let mut told = 0;
    assert_eq!(heal_first(&mut fake, || told += 1), Some(TASK_MISSING));
    assert_eq!(told, 1);
    // Tasks that are fine, can't be read, or a refresh task that can't start: never told.
    for (repair, starts) in [
        (None, true),
        (Some(TASK_UNCHECKED), true),
        (Some(TASK_MISSING), false),
    ] {
        let mut fake = Fake::new();
        fake.task_repair = repair;
        fake.heal_starts = starts;
        let mut told = 0;
        assert_eq!(heal_first(&mut fake, || told += 1), repair);
        assert_eq!(told, 0, "{repair:?}");
    }
}
