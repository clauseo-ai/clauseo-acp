//! The launch check's self-healing of WSL's deny, on the fake task and clock.

use std::time::Duration;

use super::fake::FakeTask;
use super::{DENY_WAIT, POLL, Refused, WslHealed, heal};
use crate::Blocked;
use crate::windows::refresh::WslDenyExit;
use crate::windows::refresh::last_run::LastRun;
use crate::windows::wsl::Found;

const MISSING: Result<Found, Blocked> = Err(Blocked::WslDenyMissing);
const DENIED: Result<Found, Blocked> = Ok(Found::Denied(2));

fn secs(secs: f64) -> Duration {
    Duration::from_secs_f64(secs)
}

/// The task's last run `code`, at an OLE date.
fn ran(code: WslDenyExit) -> LastRun {
    let code = i32::from(code.as_code());
    LastRun::Ran { code, at: 46_334.5 }
}

#[test]
fn a_deny_in_place_asks_nothing_and_unverifiable_is_refused_at_once() {
    let task = FakeTask::new();
    let found = heal(&mut task.checks(vec![(secs(0.0), DENIED)]), &task);
    assert_eq!(found, Ok((Found::Denied(2), WslHealed::Nothing)));
    assert_eq!(task.requests.get(), 0);
    let unverifiable = Err(Blocked::WslDenyUnverifiable);
    let found = heal(&mut task.checks(vec![(secs(0.0), unverifiable)]), &task);
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
    let found = heal(&mut checks, &task);
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
    let found = heal(&mut task.checks(vec![(secs(0.0), MISSING)]), &task);
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
    let found = heal(&mut task.checks(vec![(secs(0.0), MISSING)]), &task);
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
