//! A sweep that already ended 56 for the number a marker awaits: the launch and F4 refuse at
//! once, the fresh sweep still requested; any other last exit, or none, waits as before.

use super::*;

/// A marker of `POOL_A`'s awaiting sweep 6, with `SweepSequence` at 5 and `last` recorded.
fn awaiting_6(last: Option<(u32, u64)>) -> Fake {
    let mut fake = Fake::new();
    fake.marker(
        "a1",
        Some(POOL_A),
        Some(MarkerState::AwaitingSweep { sweep: 6 }),
    );
    fake.done = Sequences { stop: 0, sweep: 5 };
    fake.last = last;
    fake
}

#[test]
fn a_56_at_the_awaited_number_or_past_it_blocks_a_launch_at_once_with_a_sweep_asked() {
    for sequence in [6, 7] {
        let mut fake = awaiting_6(Some((56, sequence)));
        let refused = await_sweeps(&mut fake).unwrap_err();
        let Unswept::LeftoverTask(why) = &refused else {
            panic!("not a leftover task: {refused:?}");
        };
        assert!(why.contains("ended 56"), "{why}");
        assert_eq!(fake.asked("sweep"), 1, "the self-heal's sweep");
        assert_eq!(fake.asked("pause"), 0, "no 15 s wait");
        assert!(fake.has("a1"));
    }
}

#[test]
fn a_56_at_an_older_number_waits_as_before() {
    let mut fake = awaiting_6(Some((56, 5)));
    let refused = await_sweeps(&mut fake).unwrap_err();
    assert!(matches!(refused, Unswept::Waiting(_)), "{refused:?}");
    assert_eq!(fake.asked("pause"), 150);
    // And goes once the run it asked for completes.
    let mut healed = awaiting_6(Some((56, 5)));
    healed.sweeps = vec![6];
    assert_eq!(await_sweeps(&mut healed), Ok(()));
    assert!(!healed.has("a1"));
}

#[test]
fn a_last_0_or_another_exit_or_none_keeps_the_wait() {
    // A 0 run wrote `SweepSequence`: the marker is clean and the launch goes, nothing asked.
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
        let refused = await_sweeps(&mut stuck).unwrap_err();
        assert!(matches!(refused, Unswept::Waiting(_)), "{last:?}");
        assert_eq!(stuck.asked("pause"), 150, "{last:?}");
    }
}

#[test]
fn f4_refuses_a_logon_at_once_with_its_own_exit_and_keeps_awaiting_sweep_otherwise() {
    let mut fake = awaiting_6(Some((56, 6)));
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::LeftoverTask);
    assert!(refused.why.contains("ended 56"), "{refused:?}");
    assert_eq!(fake.asked("sweep"), 1);
    assert_eq!((fake.asked("pause"), fake.asked("stop")), (0, 0));
    assert_eq!(RunExit::LeftoverTask.code(), 0xE0C1_5014);
    assert_eq!(
        refused.exit.no_sandbox(),
        Some(crate::NoSandbox::SetUpBlocked(crate::Blocked::LeftoverTask))
    );
    // Not yet swept, or an older 56: `AwaitingSweep` after the wait, as before.
    for last in [None, Some((56, 5))] {
        let mut fake = awaiting_6(last);
        let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
        assert_eq!(refused.exit, RunExit::AwaitingSweep, "{last:?}");
        assert_eq!(fake.asked("pause"), 150, "{last:?}");
    }
}
