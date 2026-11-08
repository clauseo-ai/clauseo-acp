//! A sweep that already ended 56 for the number a marker awaits: the wait asks for a fresh run
//! (unless one is under way) and ends on its 0 (clean) or its 56 (a leftover task), and a 56
//! still standing at its end is a leftover task too. Any other last exit, or none, waits as
//! before.

use super::*;

/// A marker of `POOL_A`'s awaiting sweep 6, with `SweepSequence` at 5, `last` recorded and no
/// run under way (`SweepNext` one past the last run's number).
fn awaiting_6(last: Option<(u32, u64)>) -> Fake {
    let mut fake = Fake::new();
    fake.marker(
        "a1",
        Some(POOL_A),
        Some(MarkerState::AwaitingSweep { sweep: 6 }),
    );
    fake.done = Sequences { stop: 0, sweep: 5 };
    fake.last = last;
    fake.sweep_next = last.map_or(1, |(_, sequence)| sequence + 1);
    fake
}

#[test]
fn a_fresh_0_within_the_wait_lets_the_launch_go() {
    // The administrator deleted the task just before: the fresh run, 7, ends 0.
    let mut fake = awaiting_6(Some((56, 6)));
    fake.sweeps = vec![5, 7];
    fake.lasts = vec![(56, 6), (0, 7)];
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!((fake.asked("sweep"), fake.asked("pause")), (1, 2));
    assert!(!fake.has("a1"));
}

#[test]
fn a_fresh_56_refuses_as_a_leftover_task_without_the_full_wait() {
    for seen in [6, 7] {
        let mut fake = awaiting_6(Some((56, seen)));
        fake.lasts = vec![(56, seen), (56, seen + 1)];
        let refused = await_sweeps(&mut fake).unwrap_err();
        let Unswept::LeftoverTask(why) = &refused else {
            panic!("not a leftover task: {refused:?}");
        };
        assert!(why.contains("ended 56 again"), "{why}");
        assert_eq!((fake.asked("sweep"), fake.asked("pause")), (1, 2));
        assert!(fake.has("a1"));
    }
}

#[test]
fn a_wait_that_times_out_with_the_56_still_standing_refuses_as_a_leftover_task() {
    let mut fake = awaiting_6(Some((56, 6)));
    let refused = await_sweeps(&mut fake).unwrap_err();
    assert!(matches!(refused, Unswept::LeftoverTask(_)), "{refused:?}");
    assert_eq!((fake.asked("sweep"), fake.asked("pause")), (1, 150));
    // A fresh run that ended otherwise (57) doesn't stand for a leftover task.
    let mut other = awaiting_6(Some((56, 6)));
    other.lasts = vec![(57, 7)];
    let refused = await_sweeps(&mut other).unwrap_err();
    assert!(matches!(refused, Unswept::Waiting(_)), "{refused:?}");
}

#[test]
fn a_0_run_between_its_sequence_and_its_exit_is_clean_not_a_leftover() {
    // The 0 run wrote `SweepSequence = 7` (step 6) but not yet `SweepLastExit` (step 7): the
    // old 56 still reads, and the sequence, read after it, overrules it.
    let mut fake = awaiting_6(Some((56, 6)));
    fake.sweeps = vec![7];
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!(fake.asked("pause"), 1);
    // Already so before the wait: nothing asked, nothing waited.
    let mut before = awaiting_6(Some((56, 6)));
    before.done.sweep = 6;
    assert_eq!(await_sweeps(&mut before), Ok(()));
    assert_eq!(before.log, ["remove a1"]);
}

#[test]
fn a_run_under_way_isnt_asked_for_again_until_the_wait_ends() {
    // `SweepNext` 8: run 7 reserved its number and hasn't recorded its exit.
    let mut fake = awaiting_6(Some((56, 6)));
    fake.sweep_next = 8;
    fake.sweeps = vec![5, 7];
    fake.lasts = vec![(56, 6), (0, 7)];
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!(fake.asked("sweep"), 0);
    // One that never records (it died) holds back no request for good: asked once the wait ends.
    let mut stuck = awaiting_6(Some((56, 6)));
    stuck.sweep_next = 8;
    assert!(await_sweeps(&mut stuck).is_err());
    assert_eq!(stuck.asked("sweep"), 1);
    assert_eq!(stuck.log.last().map(String::as_str), Some("sweep"));
}

#[test]
fn a_56_at_an_older_number_waits_as_before() {
    let mut fake = awaiting_6(Some((56, 5)));
    let refused = await_sweeps(&mut fake).unwrap_err();
    assert!(matches!(refused, Unswept::Waiting(_)), "{refused:?}");
    assert_eq!(fake.asked("pause"), 150);
    let mut healed = awaiting_6(Some((56, 5)));
    healed.sweeps = vec![6];
    assert_eq!(await_sweeps(&mut healed), Ok(()));
    assert!(!healed.has("a1"));
}

#[test]
fn a_last_0_or_another_exit_or_none_keeps_the_wait() {
    let mut swept = awaiting_6(Some((0, 6)));
    swept.done.sweep = 6;
    assert_eq!(await_sweeps(&mut swept), Ok(()));
    assert_eq!(swept.log, ["remove a1"]);
    for last in [Some((0, 6)), Some((57, 6)), Some((59, 9)), None] {
        let mut fake = awaiting_6(last);
        fake.sweeps = vec![5, 6];
        assert_eq!(await_sweeps(&mut fake), Ok(()), "{last:?}");
        assert_eq!(fake.asked("pause"), 2, "{last:?}");
        let mut stuck = awaiting_6(last);
        // A 56 during a wait that didn't start from one changes nothing either.
        stuck.lasts = vec![(56, 6)];
        let refused = await_sweeps(&mut stuck).unwrap_err();
        assert!(matches!(refused, Unswept::Waiting(_)), "{last:?}");
        assert_eq!(stuck.asked("pause"), 150, "{last:?}");
    }
}

#[test]
fn f4_refuses_a_logon_on_a_fresh_56_with_its_own_exit_which_is_no_set_up_cause() {
    let mut fake = awaiting_6(Some((56, 6)));
    fake.lasts = vec![(56, 7)];
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::LeftoverTask);
    assert!(refused.why.contains("ended 56 again"), "{refused:?}");
    assert_eq!((fake.asked("sweep"), fake.asked("pause")), (1, 1));
    assert_eq!(fake.asked("stop"), 0);
    assert_eq!(RunExit::LeftoverTask.code(), 0xE0C1_5014);
    assert_eq!(refused.exit.no_sandbox(), None, "a pool user can cause it");
    // A fresh 0 lets the logon go.
    let mut healed = awaiting_6(Some((56, 6)));
    healed.sweeps = vec![7];
    healed.lasts = vec![(0, 7)];
    assert_eq!(logon_check(&mut healed, &sid(POOL_A)), Ok(()));
    // Not yet swept, or an older 56: `AwaitingSweep` after the wait, as before.
    for last in [None, Some((56, 5))] {
        let mut fake = awaiting_6(last);
        let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
        assert_eq!(refused.exit, RunExit::AwaitingSweep, "{last:?}");
        assert_eq!(fake.asked("pause"), 150, "{last:?}");
    }
}
