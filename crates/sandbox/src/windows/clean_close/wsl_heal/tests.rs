//! The launch check's self-healing of WSL's deny, on the fake task and clock.

use std::time::Duration;

use super::fake::FakeTask;
use std::cell::Cell;

use super::{
    DENY_WAIT, LaunchProgress, POLL, Refused, UPGRADE_RUNNING_WAIT, UPGRADE_WAIT, WslHealed, heal,
};
use crate::Blocked;
use crate::windows::refresh::last_run::{LastRun, SCHED_S_TASK_RUNNING};
use crate::windows::refresh::{WslDenyExit, WslUpgradeExit};
use crate::windows::wsl::Found;

const MISSING: Result<Found, Blocked> = Err(Blocked::WslDenyMissing);
const DENIED: Result<Found, Blocked> = Ok(Found::Denied(2));

/// A host that ignores progress.
fn none(_: LaunchProgress) {}

fn secs(secs: f64) -> Duration {
    Duration::from_secs_f64(secs)
}

/// The task's last run `code`, at an OLE date.
fn ran(code: WslDenyExit) -> LastRun {
    let code = i32::from(code.as_code());
    LastRun::Ran { code, at: 46_334.5 }
}

/// The upgrade task's last run `code`, at an OLE date.
fn ran_upgrade(code: WslUpgradeExit) -> LastRun {
    let code = i32::from(code.as_code());
    LastRun::Ran { code, at: 46_334.5 }
}

const PENDING: Result<Found, Blocked> = Ok(Found::UpgradePending(2));
/// The run the request started: running from 1 s, at a later OLE date.
const RUNNING: LastRun = LastRun::Ran {
    code: SCHED_S_TASK_RUNNING,
    at: 46_334.6,
};

/// A progress sink that counts the upgrade's notices.
fn counter(seen: &Cell<usize>) -> impl Fn(LaunchProgress) + '_ {
    move |progress| {
        assert_eq!(progress, LaunchProgress::UpdatingWindowsComponent);
        seen.set(seen.get() + 1);
    }
}

#[test]
fn a_deny_in_place_asks_nothing_and_unverifiable_is_refused_at_once() {
    let task = FakeTask::new();
    let found = heal(&mut task.checks(vec![(secs(0.0), DENIED)]), &task, &none);
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::Nothing)));
    assert_eq!(task.requests.get(), 0);
    let unverifiable = Err(Blocked::WslDenyUnverifiable);
    let found = heal(
        &mut task.checks(vec![(secs(0.0), unverifiable)]),
        &task,
        &none,
    );
    let refused = found.unwrap_err();
    assert_eq!(refused.blocked, Blocked::WslDenyUnverifiable);
    assert_eq!(
        (task.requests.get(), task.elapsed_now()),
        (0, Duration::ZERO)
    );
}

#[test]
fn a_missing_deny_is_healed_by_the_task_and_the_launch_goes_on() {
    let task = FakeTask::new();
    let mut checks = task.checks(vec![(secs(0.0), MISSING), (secs(1.2), DENIED)]);
    let found = heal(&mut checks, &task, &none);
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::DenyRestored)));
    assert_eq!(task.requests.get(), 1);
    // Polled every 250 ms: healed at the first poll after 1.2 s.
    assert_eq!(task.elapsed_now(), POLL * 5);
    assert!(WslHealed::DenyRestored.log_line().is_some());
    assert_eq!(WslHealed::Nothing.log_line(), None);
}

#[test]
fn a_deny_still_missing_after_the_wait_is_refused_with_the_task_s_last_result() {
    let mut task = FakeTask::new();
    task.last = vec![(secs(0.0), Ok(ran(WslDenyExit::NotAllowed)))];
    let found = heal(&mut task.checks(vec![(secs(0.0), MISSING)]), &task, &none);
    let Err(Refused { blocked, why }) = found else {
        panic!("{found:?}");
    };
    assert_eq!(blocked, Blocked::WslDenyMissing);
    assert!(why.contains("still so after 10 s"), "{why}");
    assert!(why.ends_with("last run: NotAllowed (51)"), "{why}");
    assert!(task.elapsed_now() >= DENY_WAIT && task.elapsed_now() < DENY_WAIT + POLL);
    assert_eq!(task.requests.get(), 1);
}

#[test]
fn a_request_that_fails_is_refused_at_once_and_says_why() {
    let task = FakeTask::absent();
    let found = heal(&mut task.checks(vec![(secs(0.0), MISSING)]), &task, &none);
    let Err(Refused { blocked, why }) = found else {
        panic!("{found:?}");
    };
    assert_eq!(blocked, Blocked::WslDenyMissing);
    assert!(
        why.starts_with("the WSL deny task couldn't be started: "),
        "{why}"
    );
    assert!(
        why.ends_with("last run: unreadable (not registered)"),
        "{why}"
    );
    assert_eq!(task.elapsed_now(), Duration::ZERO);
}

#[test]
fn a_pending_upgrade_is_installed_the_deny_reapplied_then_the_launch_goes_on() {
    let task = FakeTask::new();
    let seen = Cell::new(0);
    // The MSI's rewrite drops the deny at 30 s; the task's phase puts it back at 32 s.
    let script = vec![
        (secs(0.0), PENDING),
        (secs(30.0), MISSING),
        (secs(32.0), DENIED),
    ];
    let found = heal(&mut task.checks(script), &task, &counter(&seen));
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::Upgraded)));
    // The upgrade task alone was asked: the deny task never installs it.
    let asked = (task.requests.get(), task.upgrade_requests.get());
    assert_eq!((seen.get(), asked), (1, (0, 1)));
    assert_eq!(task.elapsed_now(), secs(32.0));
    assert!(
        WslHealed::Upgraded
            .log_line()
            .unwrap()
            .contains("lawyer's behalf")
    );
}

#[test]
fn a_failed_upgrade_with_the_deny_applied_launches_once_the_task_says_so() {
    let mut task = FakeTask::new();
    let code = i32::from(WslUpgradeExit::UpgradeFailed.as_code());
    let failed = LastRun::Ran { code, at: 46_334.6 };
    task.upgrade_last = vec![
        (secs(0.0), Ok(ran_upgrade(WslUpgradeExit::Done))),
        (secs(1.0), Ok(RUNNING)),
        (secs(40.0), Ok(failed)),
    ];
    let seen = Cell::new(0);
    let found = heal(
        &mut task.checks(vec![(secs(0.0), PENDING)]),
        &task,
        &counter(&seen),
    );
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::UpgradeFailed)));
    assert_eq!(task.elapsed_now(), secs(40.0));
    assert_eq!(seen.get(), 1);
    // A 62 from before the request is no answer: the wait runs out, and the deny verified
    // still launches.
    let mut task = FakeTask::new();
    let failed = ran_upgrade(WslUpgradeExit::UpgradeFailed);
    task.upgrade_last = vec![(secs(0.0), Ok(failed))];
    let found = heal(&mut task.checks(vec![(secs(0.0), PENDING)]), &task, &none);
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::UpgradeUnfinished)));
    assert!(task.elapsed_now() >= UPGRADE_WAIT);
}

#[test]
fn a_run_that_ends_without_the_upgrade_or_no_task_still_launches_with_the_deny_verified() {
    let mut task = FakeTask::new();
    let done = LastRun::Ran {
        code: 0,
        at: 46_334.6,
    };
    task.upgrade_last = vec![(secs(0.0), Ok(LastRun::NeverRun)), (secs(2.0), Ok(done))];
    let found = heal(&mut task.checks(vec![(secs(0.0), PENDING)]), &task, &none);
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::UpgradeUnfinished)));
    assert_eq!(task.elapsed_now(), secs(2.0));
    let task = FakeTask::absent();
    let found = heal(&mut task.checks(vec![(secs(0.0), PENDING)]), &task, &none);
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::UpgradeUnfinished)));
    assert_eq!(task.elapsed_now(), Duration::ZERO);
}

#[test]
fn a_deny_unverifiable_at_the_end_of_an_upgrade_is_refused() {
    let task = FakeTask::new();
    let script = vec![(secs(0.0), PENDING), (secs(30.0), MISSING)];
    let found = heal(&mut task.checks(script), &task, &none);
    let Err(Refused { blocked, why }) = found else {
        panic!("{found:?}");
    };
    assert_eq!(blocked, Blocked::WslDenyMissing);
    assert!(why.contains("still so after 120 s"), "{why}");
    assert!(task.elapsed_now() >= UPGRADE_WAIT && task.elapsed_now() < UPGRADE_WAIT + POLL);
    let task = FakeTask::new();
    let unverifiable = Err(Blocked::WslDenyUnverifiable);
    let script = vec![(secs(0.0), PENDING), (secs(30.0), unverifiable)];
    let found = heal(&mut task.checks(script), &task, &none);
    assert_eq!(found.unwrap_err().blocked, Blocked::WslDenyUnverifiable);
}

#[test]
fn a_missing_deny_that_turns_into_a_pending_upgrade_waits_for_the_upgrade() {
    let task = FakeTask::new();
    let seen = Cell::new(0);
    let script = vec![
        (secs(0.0), MISSING),
        (secs(1.0), PENDING),
        (secs(50.0), DENIED),
    ];
    let found = heal(&mut task.checks(script), &task, &counter(&seen));
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::Upgraded)));
    assert_eq!((seen.get(), task.elapsed_now()), (1, secs(50.0)));
}

#[test]
fn a_run_still_going_is_waited_for_past_the_upgrade_wait_then_refused_at_its_limit() {
    assert_eq!(
        UPGRADE_RUNNING_WAIT,
        crate::windows::wsl::MSI_WAIT + secs(30.0)
    );
    // Still installing at 120 s: the wait goes on; the run ends at 200 s, installed.
    let mut task = FakeTask::new();
    let done = LastRun::Ran {
        code: 0,
        at: 46_334.6,
    };
    task.upgrade_last = vec![
        (secs(0.0), Ok(LastRun::NeverRun)),
        (secs(1.0), Ok(RUNNING)),
        (secs(200.0), Ok(done)),
    ];
    let seen = Cell::new(0);
    let script = vec![(secs(0.0), PENDING), (secs(199.0), DENIED)];
    let found = heal(&mut task.checks(script), &task, &counter(&seen));
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::Upgraded)));
    assert_eq!((seen.get(), task.elapsed_now()), (1, secs(200.0)));
    // Still running at the limit: refused, never launched mid-install.
    let mut task = FakeTask::new();
    task.upgrade_last = vec![(secs(0.0), Ok(LastRun::NeverRun)), (secs(1.0), Ok(RUNNING))];
    let found = heal(&mut task.checks(vec![(secs(0.0), PENDING)]), &task, &none);
    let Err(Refused { blocked, why }) = found else {
        panic!("{found:?}");
    };
    assert_eq!(blocked, Blocked::WslUpgradePending);
    assert!(why.contains("still running after 270 s"), "{why}");
    let at = task.elapsed_now();
    assert!(
        at >= UPGRADE_RUNNING_WAIT && at < UPGRADE_RUNNING_WAIT + POLL,
        "{at:?}"
    );
}

#[test]
fn an_upgrade_installed_while_its_run_still_goes_launches_only_once_the_run_ends() {
    let mut task = FakeTask::new();
    let done = LastRun::Ran {
        code: 0,
        at: 46_334.6,
    };
    task.upgrade_last = vec![
        (secs(0.0), Ok(LastRun::NeverRun)),
        (secs(1.0), Ok(RUNNING)),
        (secs(45.0), Ok(done)),
    ];
    // The MSI's version is current from 30 s, the deny verified, but the run goes on to 45 s.
    let script = vec![(secs(0.0), PENDING), (secs(30.0), DENIED)];
    let found = heal(&mut task.checks(script), &task, &none);
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::Upgraded)));
    assert_eq!(task.elapsed_now(), secs(45.0));
}

#[test]
fn a_deferred_upgrade_launches_on_the_current_wsl_with_the_deny_verified() {
    let mut task = FakeTask::new();
    let deferred = LastRun::Ran {
        code: i32::from(WslUpgradeExit::Deferred.as_code()),
        at: 46_334.6,
    };
    task.upgrade_last = vec![
        (secs(0.0), Ok(LastRun::NeverRun)),
        (secs(1.0), Ok(RUNNING)),
        (secs(3.0), Ok(deferred)),
    ];
    let found = heal(&mut task.checks(vec![(secs(0.0), PENDING)]), &task, &none);
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::UpgradeDeferred)));
    assert_eq!(task.elapsed_now(), secs(3.0));
    assert!(
        WslHealed::UpgradeDeferred
            .log_line()
            .unwrap()
            .contains("deferred")
    );
    // Deferred with the deny missing is still refused: the deny is verified either way.
    let mut task = FakeTask::new();
    task.upgrade_last = vec![
        (secs(0.0), Ok(LastRun::NeverRun)),
        (secs(3.0), Ok(deferred)),
    ];
    let script = vec![(secs(0.0), PENDING), (secs(2.0), MISSING)];
    let found = heal(&mut task.checks(script), &task, &none);
    assert_eq!(found.unwrap_err().blocked, Blocked::WslDenyMissing);
}

#[test]
fn a_missing_deny_asks_the_deny_task_and_a_pending_upgrade_the_upgrade_task() {
    let task = FakeTask::new();
    let found = heal(
        &mut task.checks(vec![(secs(0.0), MISSING), (secs(1.0), DENIED)]),
        &task,
        &none,
    );
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::DenyRestored)));
    assert_eq!((task.requests.get(), task.upgrade_requests.get()), (1, 0));
    let task = FakeTask::new();
    let script = vec![
        (secs(0.0), MISSING),
        (secs(1.0), PENDING),
        (secs(50.0), DENIED),
    ];
    heal(&mut task.checks(script), &task, &none).unwrap();
    assert_eq!((task.requests.get(), task.upgrade_requests.get()), (1, 1));
}
