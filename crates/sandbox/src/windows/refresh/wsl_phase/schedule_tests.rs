//! `--wsl-deny`'s schedule (`schedule.rs`) on the fake machine's clock, keys and installer.

use std::time::Duration;

use super::super::fake::{Fake, good, key, new, old};
use super::super::fake_watch::{KeyEvent, Keys};
use super::super::{WSL_DENY_EXECUTION_LIMIT, WSL_LOCK_WAIT};
use super::{
    WSL_DENY_BUDGET, WSL_INSTALL_QUIET, WSL_RECHECK_PAUSE, WSL_WAKE_GAP, WSL_WAKE_SETTLE,
    WslDenyExit, wsl_deny,
};

/// The fake machine, ready for the run.
fn fake() -> Fake {
    Fake::new(&old(), &new(), &key(), &good())
}

fn secs(secs: f64) -> Duration {
    Duration::from_secs_f64(secs)
}

/// Runs `--wsl-deny` on `machine`, which must end it with `Done` and never look at the pool.
fn run(machine: &mut Fake) {
    assert_eq!(wsl_deny(machine), WslDenyExit::Done);
    assert!(!machine.calls.contains(&"pool_busy"));
    assert!(!machine.calls.contains(&"upgrade_wsl"));
}

/// The error lines `machine` logged.
fn errors(machine: &Fake) -> usize {
    machine.logs.iter().filter(|(error, _)| *error).count()
}

#[test]
fn a_rewrite_at_21_s_wakes_the_run_and_is_denied_again_within_the_settle() {
    let mut machine = fake();
    // WSL's installer as the VM saw it: keys deleted ~5 s in, written back ~21 s in.
    let events = vec![
        (secs(5.0), KeyEvent::Delete),
        (secs(21.0), KeyEvent::Rewrite),
    ];
    machine.watch.keys = Some(Keys::new(events));
    machine.watch.in_progress = vec![(secs(0.0), secs(26.0))];
    run(&mut machine);
    let keys = machine.watch.keys.as_ref().unwrap();
    assert!(keys.present && keys.denied);
    // The deny was off for the settle alone, not until the next timed pass.
    assert_eq!(keys.off, [WSL_WAKE_SETTLE]);
    assert!(machine.watch.passes.contains(&secs(21.25)));
    // The deletion's wake found the keys gone: one error line, not one a pass.
    assert!(machine.watch.passes.contains(&secs(5.25)));
    assert_eq!(errors(&machine), 1, "{:?}", machine.logs);
    assert_eq!(machine.logs.len(), 2, "{:?}", machine.logs);
    // It ended once InProgress had been gone 3 s and a last pass verified the deny.
    let last = *machine.watch.passes.last().unwrap();
    assert!(last >= secs(26.0) + WSL_INSTALL_QUIET, "{last:?}");
    assert_eq!(machine.watch.clock, last);
    assert!(machine.watch.clock < secs(31.0));
}

#[test]
fn in_progress_starts_and_ends_the_hold_and_a_verified_pass_ends_the_run() {
    let mut machine = fake();
    machine.watch.in_progress = vec![(secs(10.0), secs(40.0))];
    run(&mut machine);
    // The floor's five passes, then the final one, 3 s after InProgress went.
    let floor = [0.0, 5.0, 10.0, 15.0, 20.0].map(secs);
    assert_eq!(machine.watch.passes, [&floor[..], &[secs(43.0)]].concat());
    assert_eq!(machine.watch.clock, secs(43.0));
    assert!(machine.logs.is_empty(), "{:?}", machine.logs);

    // A deny that doesn't verify when the install is over keeps the run: a key gone after
    // InProgress ended is retried, and its return wakes the run, which then ends.
    let mut machine = machine_with_keys(vec![
        (secs(35.0), KeyEvent::Delete),
        (secs(47.0), KeyEvent::Rewrite),
    ]);
    machine.watch.in_progress = vec![(secs(0.0), secs(40.0))];
    run(&mut machine);
    let keys = machine.watch.keys.as_ref().unwrap();
    assert!(keys.denied);
    assert_eq!(keys.off, [WSL_WAKE_SETTLE]);
    assert!(machine.watch.passes.contains(&secs(43.25)));
    assert_eq!(machine.watch.clock, secs(47.25));
    assert_eq!(errors(&machine), 1, "{:?}", machine.logs);
}

fn machine_with_keys(events: Vec<(Duration, KeyEvent)>) -> Fake {
    let mut machine = fake();
    machine.watch.keys = Some(Keys::new(events));
    machine
}

#[test]
fn with_no_in_progress_a_recorded_key_missing_holds_the_run_past_the_rewrite() {
    // InProgress at neither path: the deleted keys alone keep the run past its floor.
    let mut machine = machine_with_keys(vec![
        (secs(5.0), KeyEvent::Delete),
        (secs(21.0), KeyEvent::Rewrite),
    ]);
    run(&mut machine);
    let keys = machine.watch.keys.as_ref().unwrap();
    assert!(keys.present && keys.denied);
    assert_eq!(keys.off, [WSL_WAKE_SETTLE]);
    assert_eq!(machine.watch.clock, secs(21.25));
    assert_eq!(errors(&machine), 1, "{:?}", machine.logs);
}

#[test]
fn the_mutex_without_in_progress_holds_nothing_and_is_logged() {
    let mut machine = fake();
    machine.watch.mutex = vec![(secs(0.0), secs(1000.0))];
    run(&mut machine);
    assert_eq!(machine.watch.passes.len(), 5);
    assert_eq!(machine.watch.clock, secs(20.0));
    assert_eq!(machine.logs.len(), 1, "{:?}", machine.logs);
    let (error, line) = &machine.logs[0];
    assert!(
        !error && line.contains("mutex held with no install in progress"),
        "{line}"
    );
}

#[test]
fn with_no_install_the_run_is_the_old_one_and_a_change_adds_a_pass() {
    let mut machine = machine_with_keys(Vec::new());
    run(&mut machine);
    let floor = [0.0, 5.0, 10.0, 15.0, 20.0].map(secs);
    assert_eq!(machine.watch.passes, floor);
    assert_eq!(machine.watch.waits, [WSL_RECHECK_PAUSE; 4]);
    assert!(machine.logs.is_empty(), "{:?}", machine.logs);

    // A change during the floor gets a pass of its own; the floor and the end stay.
    let mut machine = machine_with_keys(vec![(secs(7.0), KeyEvent::Touch)]);
    run(&mut machine);
    let passes = [0.0, 5.0, 7.25, 10.0, 15.0, 20.0].map(secs);
    assert_eq!(machine.watch.passes, passes);
    assert_eq!(machine.paused, [WSL_WAKE_SETTLE]);
}

#[test]
fn a_change_made_while_a_pass_reads_is_caught_by_the_notification_armed_before_it() {
    // A change wakes the run at 21 s; while that pass reads (finding the deny in place), the
    // installer writes the keys again. The notification was armed before the read, so it fires.
    let mut machine = machine_with_keys(vec![(secs(21.0), KeyEvent::Touch)]);
    machine.watch.keys.as_mut().unwrap().mid_read = vec![(secs(21.0), KeyEvent::Rewrite)];
    machine.watch.in_progress = vec![(secs(0.0), secs(25.0))];
    run(&mut machine);
    let keys = machine.watch.keys.as_ref().unwrap();
    assert!(keys.denied);
    // Put back by the next pass, one wake gap after the one that missed it.
    assert_eq!(keys.off, [WSL_WAKE_GAP]);
    assert!(machine.watch.passes.contains(&secs(22.25)));
    // Every pass reads only after its arm.
    let wanted = ["watch", "wait_change", "reapply_wsl"];
    let order: Vec<&str> = machine
        .calls
        .iter()
        .copied()
        .filter(|call| wanted.contains(call))
        .collect();
    for (at, call) in order.iter().enumerate() {
        if *call == "reapply_wsl" {
            assert_eq!(order.get(at.wrapping_sub(1)), Some(&"watch"), "{order:?}");
        }
    }
}

#[test]
fn a_storm_of_changes_gives_at_most_one_pass_a_second() {
    // With no install, the floor still ends the run at 20 s.
    let mut machine = fake();
    machine.watch.storm_until = secs(1000.0);
    run(&mut machine);
    assert!(
        machine.watch.clock <= secs(21.0),
        "{:?}",
        machine.watch.clock
    );
    let most = 5 + 21;
    assert!(
        machine.watch.passes.len() <= most,
        "{}",
        machine.watch.passes.len()
    );
    for pair in machine.watch.passes.windows(2) {
        assert!(pair[1] > pair[0]);
    }

    // During an install, the storm runs to the budget, still one pass a second at most.
    let mut machine = fake();
    machine.watch.storm_until = secs(1000.0);
    machine.watch.in_progress = vec![(secs(0.0), secs(1000.0))];
    run(&mut machine);
    assert!(machine.watch.clock <= WSL_DENY_BUDGET);
    let gap = usize::try_from(WSL_DENY_BUDGET.as_secs() / WSL_WAKE_GAP.as_secs()).unwrap();
    assert!(machine.watch.passes.len() <= 5 + gap + 1);
    let woken: Vec<&Duration> = machine.watch.passes.iter().skip(5).collect();
    for pair in woken.windows(2) {
        assert!(pair[1].saturating_sub(*pair[0]) >= WSL_WAKE_GAP, "{pair:?}");
    }
}

#[test]
fn an_install_that_never_ends_stops_the_run_at_its_budget() {
    let mut machine = fake();
    machine.watch.in_progress = vec![(secs(0.0), secs(10_000.0))];
    run(&mut machine);
    assert!(
        machine.watch.clock <= WSL_DENY_BUDGET,
        "{:?}",
        machine.watch.clock
    );
    // No pass starts so late that its waits could carry it past the budget.
    let last = *machine.watch.passes.last().unwrap();
    assert!(
        last + WSL_WAKE_GAP + WSL_LOCK_WAIT <= WSL_DENY_BUDGET,
        "{last:?}"
    );
    let (error, line) = machine.logs.last().unwrap();
    assert!(!error && line.contains("time limit"), "{line}");
    assert!(line.ends_with("still in progress"), "{line}");
    // A deny left unverified at the budget (a recorded key that never comes back) is an error.
    let mut machine = machine_with_keys(vec![(secs(5.0), KeyEvent::Delete)]);
    run(&mut machine);
    assert!(
        machine.watch.clock <= WSL_DENY_BUDGET,
        "{:?}",
        machine.watch.clock
    );
    let (error, line) = machine.logs.last().unwrap();
    assert!(
        *error && line.ends_with("WSL's deny wasn't verified"),
        "{line}"
    );
    assert_eq!(errors(&machine), 2, "{:?}", machine.logs);
    // The budget sits 30 s inside the task's limit.
    assert_eq!(WSL_DENY_EXECUTION_LIMIT, "PT6M");
    assert_eq!(
        WSL_DENY_BUDGET + Duration::from_secs(30),
        Duration::from_secs(360)
    );
}

#[test]
fn a_notification_that_cant_be_armed_leaves_the_timed_passes() {
    let mut machine = fake();
    machine.watch.arm_fails = true;
    machine.watch.in_progress = vec![(secs(0.0), secs(12.0))];
    run(&mut machine);
    assert_eq!(machine.calls.iter().filter(|c| **c == "watch").count(), 1);
    assert_eq!(errors(&machine), 1, "{:?}", machine.logs);
    assert!(
        machine.logs[0].1.contains("aren't watched"),
        "{:?}",
        machine.logs
    );
    // The floor, then the end: InProgress gone at 12 s, quiet by the last floor pass.
    assert_eq!(machine.watch.passes, [0.0, 5.0, 10.0, 15.0, 20.0].map(secs));
}
