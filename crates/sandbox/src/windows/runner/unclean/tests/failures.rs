//! F4 when a marker's file won't change: a clean one that can't be removed refuses nothing,
//! and one whose sweep before its final stop completed but can't be re-marked refuses with no
//! stop requested. And a sweep that can't finish because its task is missing: the refusal says
//! so, in the readiness check's words.

use super::*;
use crate::windows::refresh::{TASK_CHANGED, TASK_MISSING, TASK_UNCHECKED};

const SWEPT: MarkerState = MarkerState::AwaitingSweep { sweep: 7 };

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
        // F4's logon refusal, after its 15 s wait.
        let mut fake = Fake::new();
        fake.marker("a1", Some(POOL_A), awaiting);
        fake.task_repair = Some(words);
        let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
        assert_eq!(refused, Refused::sweep(words.to_owned()));
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
