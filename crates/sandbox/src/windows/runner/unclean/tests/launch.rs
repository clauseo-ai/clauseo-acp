//! A launch's check of the markers (`await_sweeps`): a completed sweep clears without a wait,
//! one awaited is asked for and waited on, bounded, and a stop-awaiting marker holds a launch
//! only once its stop completed. An escapee's marker clears only on a sweep after its stop, and
//! one whose stop completed but can't be re-marked refuses the launch.

use super::*;

#[test]
fn a_launch_clears_a_marker_whose_sweep_completed_without_waiting() {
    let mut fake = Fake::new();
    fake.marker(
        "a1",
        Some(POOL_A),
        Some(MarkerState::AwaitingSweep { sweep: 4 }),
    );
    fake.marker("a2", Some(POOL_A), Some(MarkerState::Open));
    fake.done = Sequences { stop: 0, sweep: 4 };
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert!(!fake.has("a1"));
    // An open marker may be a live session's: left alone, and nothing asked for.
    assert_eq!(fake.state("a2"), Some(MarkerState::Open));
    assert_eq!(fake.log, ["remove a1"]);
}

#[test]
fn a_launch_asks_for_a_sweep_and_waits_for_it_bounded() {
    let mut fake = Fake::new();
    fake.marker(
        "a1",
        Some(POOL_A),
        Some(MarkerState::AwaitingSweep { sweep: 6 }),
    );
    fake.marker(
        "b1",
        Some(POOL_B),
        Some(MarkerState::AwaitingSweep { sweep: 7 }),
    );
    fake.done = Sequences { stop: 0, sweep: 5 };
    // A run completes 6, then 7, on the second and third looks.
    fake.sweeps = vec![5, 6, 7];
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!(fake.asked("sweep"), 1);
    assert_eq!(fake.asked("pause"), 3);
    assert!(fake.markers.is_empty(), "{:?}", fake.markers);
    // A sweep that never completes: 150 looks, 15 s, then the refusal; the marker stays.
    let mut stuck = Fake::new();
    stuck.marker(
        "a1",
        Some(POOL_A),
        Some(MarkerState::AwaitingSweep { sweep: 6 }),
    );
    let refused = await_sweeps(&mut stuck).unwrap_err();
    assert!(refused.contains("didn't finish within 15 s"), "{refused}");
    assert_eq!(stuck.asked("pause"), 150);
    assert_eq!(SWEEP_WAIT, Duration::from_secs(15));
    assert!(
        SWEEP_WAIT < Duration::from_secs(180),
        "inside the host's launch bound"
    );
    assert!(stuck.has("a1"));
}

#[test]
fn a_stop_awaiting_marker_holds_a_launch_only_once_its_stop_completed() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 0, sweep: 2 };
    // Its stop is F4's: the launch goes, the marker stays, and no sweep number is read.
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!(fake.state("a1"), Some(STOP));
    assert!(fake.log.is_empty(), "{:?}", fake.log);
    // Sequences that can't be read refuse.
    fake.unreadable = true;
    assert!(await_sweeps(&mut fake).is_err());
}

#[test]
fn a_sweep_before_the_stop_doesnt_clear_an_escapees_marker_and_one_after_it_does() {
    let mut fake = Fake::new();
    let escapee = MarkerState::AwaitingStop { stop: 5 };
    fake.marker("a1", Some(POOL_A), Some(escapee));
    // Sweeps complete while the escapee lives: it could register a task after any of them.
    fake.done = Sequences { stop: 4, sweep: 50 };
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert_eq!(
        fake.state("a1"),
        Some(escapee),
        "not cleared by an earlier sweep"
    );
    assert_eq!(fake.asked("read SweepNext"), 0);
    // The stop completes, the escapee dead. `SweepNext` is read now: 51, past every sweep so far.
    fake.done.stop = 5;
    fake.sweep_next = 51;
    fake.log.clear();
    // The launch re-marks it, requests the sweep once, and waits for it. The marker then owes
    // its final stop, which is F4's.
    fake.sweeps = vec![50, 51];
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    let swept = MarkerState::AwaitingSweepBeforeStop { sweep: 51 };
    assert_eq!(
        fake.state("a1"),
        Some(swept),
        "not cleared by the sweep alone"
    );
    let first = ["read SweepNext", "sweep", "pause", "pause"];
    assert_eq!(fake.log, first);
    // F4 reads the final stop's number only now and requests that stop, which clears it.
    fake.stop_next = 6;
    assert!(logon_check(&mut fake, &sid(POOL_A)).is_err());
    let last = MarkerState::AwaitingFinalStop { stop: 6 };
    assert_eq!(fake.state("a1"), Some(last));
    assert_eq!(
        await_sweeps(&mut fake),
        Ok(()),
        "a final stop doesn't hold a launch"
    );
    assert!(fake.has("a1"));
    fake.done.stop = 6;
    assert_eq!(await_sweeps(&mut fake), Ok(()));
    assert!(!fake.has("a1"), "cleared by the final stop");
}

#[test]
fn a_marker_whose_stop_completed_but_cant_be_re_marked_refuses_a_launch() {
    let mut fake = Fake::new();
    fake.marker("a1", Some(POOL_A), Some(STOP));
    fake.done = Sequences { stop: 3, sweep: 99 };
    fake.sweep_next_fails = true;
    let refused = await_sweeps(&mut fake).unwrap_err();
    assert!(refused.contains("completed stop"), "{refused}");
    assert_eq!(fake.state("a1"), Some(STOP));
    // F4 refuses the logon too, without asking for the stop, which has completed.
    let refused = logon_check(&mut fake, &sid(POOL_A)).unwrap_err();
    assert_eq!(refused.exit, RunExit::AwaitingSweep, "no stop is awaited");
    assert!(refused.why.contains("completed stop"), "{refused:?}");
    assert_eq!(fake.state("a1"), Some(STOP));
    assert_eq!(fake.asked("stop"), 0);
}
