//! WSL's self-healing at launch (ADR 0049, "WSL", "Self-healing"). The product rule: a lawyer is
//! never refused the Safe workspace for a state Clauseo can repair itself; a launch is refused
//! only when the repair really failed, and the log says why.
//!
//! A deny found missing (`Blocked::WslDenyMissing`: a WSL update or repair rewrote a launch
//! permission, say) asks `\Clauseo\SandboxWslDeny` to run now, as the lawyer
//! (`refresh::task::request_wsl_deny`; the task's descriptor gives the owner read and execute,
//! `FRFX`, which starts it), then checks again every [`POLL`] for up to [`DENY_WAIT`]: the launch
//! goes on once the deny is in place. Still missing then, or a request that failed, refuses with
//! `WslDenyMissing`, and the log line gives the task's last result (`last_wsl_deny_run`).
//! `WslDenyUnverifiable` is refused at once: no task run makes an unreadable value readable.
//!
//! A pending WSL upgrade (`wsl::inspect`'s [`Found::UpgradePending`]: every deny in place, the
//! Store package newer than the MSI) asks the same task, whose SYSTEM run installs WSL's own
//! pending MSI upgrade and then re-applies the deny (`refresh::wsl_phase`, `wsl/msi.rs`). The
//! host is told first ([`LaunchProgress::UpdatingWindowsComponent`]), then the check is made
//! again every [`POLL`] for up to [`UPGRADE_WAIT`] (an MSI upgrade takes tens of seconds; a deny
//! the upgrade dropped meanwhile is waited for too). The launch goes on once the package is no
//! longer newer and the deny is in place ([`WslHealed::Upgraded`]); or once a run of the task
//! that began after the request has ended while the upgrade is still pending, the deny verified:
//! "upgrade failed, deny applied" (`WslDenyExit::UpgradeFailed`, [`WslHealed::UpgradeFailed`])
//! or any other end ([`WslHealed::UpgradeUnfinished`]); or at the end of the wait with the deny
//! verified and no run going on ([`WslHealed::UpgradeUnfinished`]). While the task's run is still
//! going, the launch never starts, installed or not: msiexec rewrites the launch permissions
//! mid-install. So the wait goes on past [`UPGRADE_WAIT`] for as long as the run goes on, up to
//! [`UPGRADE_RUNNING_WAIT`] (msiexec's own limit and 30 s), and a run still going then refuses
//! the launch (`WslUpgradePending`). A request that fails launches too, the deny being
//! verified. It is refused when the deny can't be verified at the end: missing
//! (`WslDenyMissing`) or unverifiable (`WslDenyUnverifiable`); or with the run still going after
//! [`UPGRADE_RUNNING_WAIT`] (`WslUpgradePending`). What remains is a
//! residual: a pool user could start the same upgrade through `WslInstaller` during the session
//! (its launch permission is the package's, `wsl::upgrade`), and the deny is off from the MSI's
//! rewrite until the WSL deny task's run its events start.

use std::time::Duration;

use crate::Blocked;
use crate::windows::refresh::WslDenyExit;
use crate::windows::refresh::last_run::{LastRun, SCHED_S_TASK_RUNNING};
use crate::windows::wsl::{Found, MSI_WAIT};

/// How often the check is made again while the task runs.
pub const POLL: Duration = Duration::from_millis(250);
/// How long the launch waits for a missing deny: the task's run takes a moment.
pub const DENY_WAIT: Duration = Duration::from_secs(10);
/// How long the launch waits for a pending upgrade: msiexec takes tens of seconds.
pub const UPGRADE_WAIT: Duration = Duration::from_secs(120);
/// How long the launch waits for a pending upgrade while the task's run is still going: the
/// run's wait for msiexec ([`MSI_WAIT`]) and 30 s for the deny after it. The MSI rewrites the
/// launch permissions mid-install, so no launch starts while the run may still be installing.
pub const UPGRADE_RUNNING_WAIT: Duration = Duration::from_secs(MSI_WAIT.as_secs() + 30);

pub use super::wsl_healed::{LaunchProgress, WslHealed};

/// The WSL deny task, as the launch check uses it: the Task Scheduler on Windows, a fake in the
/// tests. The clock is the seam's too, so the tests' waits take no time.
pub(crate) trait DenyTask {
    /// Asks the task to run now (`IRegisteredTask::Run`, as the lawyer).
    fn request(&self) -> Result<(), String>;
    /// The task's last run.
    fn last_run(&self) -> Result<LastRun, String>;
    /// Waits `time`.
    fn sleep(&self, time: Duration);
    /// The time since the seam was made.
    fn elapsed(&self) -> Duration;
}

/// The Task Scheduler's `\Clauseo\SandboxWslDeny`, and this machine's clock.
#[cfg(windows)]
pub(crate) struct WindowsDenyTask(std::time::Instant);

#[cfg(windows)]
impl WindowsDenyTask {
    #[expect(
        clippy::disallowed_methods,
        reason = "the launch check's clock seam: the healing's core takes its time from here"
    )]
    pub(crate) fn new() -> WindowsDenyTask {
        WindowsDenyTask(std::time::Instant::now())
    }
}

#[cfg(windows)]
impl DenyTask for WindowsDenyTask {
    fn request(&self) -> Result<(), String> {
        crate::windows::refresh::task::request_wsl_deny().map_err(|error| error.to_string())
    }

    fn last_run(&self) -> Result<LastRun, String> {
        crate::windows::refresh::task::last_wsl_deny_run().map_err(|error| error.to_string())
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the launch check's clock seam: the wait for the task, off the UI thread"
    )]
    fn sleep(&self, time: Duration) {
        std::thread::sleep(time);
    }

    fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }
}

/// A refusal, and why, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Refused {
    pub(crate) blocked: Blocked,
    pub(crate) why: String,
}

/// The WSL check (`check`, `wsl::inspect` on the machine), healed as the module says: what it
/// found ([`Found::UpgradePending`] given as [`Found::Denied`]) and what the healing did, or the
/// refusal and why. `progress` hears when the wait for an upgrade begins.
pub(crate) fn heal(
    check: &mut dyn FnMut() -> Result<Found, Blocked>,
    task: &dyn DenyTask,
    progress: &dyn Fn(LaunchProgress),
) -> Result<(Found, WslHealed), Refused> {
    let mut now = check();
    let mut upgrading = match now {
        Ok(Found::UpgradePending(_)) => true,
        Err(Blocked::WslDenyMissing) => false,
        Ok(found) => return Ok((found, WslHealed::Nothing)),
        Err(blocked) => {
            let why = "not something the WSL deny task repairs".to_owned();
            return Err(Refused { blocked, why });
        }
    };
    if upgrading {
        progress(LaunchProgress::UpdatingWindowsComponent);
    }
    let before = task.last_run().ok();
    if let Err(why) = task.request() {
        let why = format!("the WSL deny task couldn't be started: {why}");
        return settle(now, &why, task);
    }
    let start = task.elapsed();
    loop {
        let wait = if upgrading { UPGRADE_WAIT } else { DENY_WAIT };
        let waited = task.elapsed().saturating_sub(start);
        if waited >= wait {
            // A pending upgrade whose run is still going is waited for, up to its own limit.
            if !upgrading || !running(task) {
                let why = format!("still so after {} s", wait.as_secs());
                return settle(now, &why, task);
            }
            if waited >= UPGRADE_RUNNING_WAIT {
                let secs = UPGRADE_RUNNING_WAIT.as_secs();
                let last = last_run_text(task);
                let why = format!("the WSL deny task was still running after {secs} s: {last}");
                return Err(Refused {
                    blocked: Blocked::WslUpgradePending,
                    why,
                });
            }
        }
        task.sleep(POLL);
        now = check();
        match now {
            Ok(Found::UpgradePending(targets)) => {
                if !upgrading {
                    upgrading = true;
                    progress(LaunchProgress::UpdatingWindowsComponent);
                }
                match ended_since(task, before) {
                    None => {}
                    Some(code) if code == i32::from(WslDenyExit::UpgradeFailed.as_code()) => {
                        let last = last_run_text(task);
                        tracing::warn!(%last, "WSL's pending update failed; the deny is verified");
                        return Ok((Found::Denied(targets), WslHealed::UpgradeFailed));
                    }
                    Some(_) => {
                        let why = "the WSL deny task's run ended without installing it";
                        return settle(now, why, task);
                    }
                }
            }
            // Installed, the deny in place: launched once no run may still be installing.
            Ok(found) if upgrading => {
                if ended_since(task, before).is_some() || !running(task) {
                    return Ok((found, WslHealed::Upgraded));
                }
            }
            Ok(found) => return Ok((found, WslHealed::DenyRestored)),
            Err(_) => {}
        }
    }
}

/// The code of a run of the task that began after `before` (the last run read before the
/// request) and has ended, if there is one: one whose run time differs, or any end when a run
/// was going on then (the request was dropped for it).
fn ended_since(task: &dyn DenyTask, before: Option<LastRun>) -> Option<i32> {
    let Ok(LastRun::Ran { code, at }) = task.last_run() else {
        return None;
    };
    let fresh = match before {
        Some(LastRun::Ran {
            code: then,
            at: was,
        }) => then == SCHED_S_TASK_RUNNING || was.to_bits() != at.to_bits(),
        Some(LastRun::NeverRun) | None => true,
    };
    (fresh && code != SCHED_S_TASK_RUNNING).then_some(code)
}

/// Whether the task's last run is still going (`SCHED_S_TASK_RUNNING`).
fn running(task: &dyn DenyTask) -> bool {
    matches!(task.last_run(), Ok(LastRun::Ran { code, .. }) if code == SCHED_S_TASK_RUNNING)
}

/// The end of a wait that didn't heal: an upgrade still pending with every deny in place goes
/// on ([`WslHealed::UpgradeUnfinished`], logged with `why`); anything else is refused, `why`
/// followed by the task's last result.
fn settle(
    now: Result<Found, Blocked>,
    why: &str,
    task: &dyn DenyTask,
) -> Result<(Found, WslHealed), Refused> {
    let last = last_run_text(task);
    match now {
        Ok(Found::UpgradePending(targets)) => {
            let line = "WSL's pending update wasn't installed; the deny is verified";
            tracing::warn!(%why, %last, "{line}");
            Ok((Found::Denied(targets), WslHealed::UpgradeUnfinished))
        }
        Ok(found) => Ok((found, WslHealed::Nothing)),
        Err(blocked) => {
            let why = format!("{why}; the WSL deny task's last run: {last}");
            Err(Refused { blocked, why })
        }
    }
}

/// The task's last run, as a log's words: its [`WslDenyExit`] and code, `running`, `never run`,
/// another code in hex, or why it couldn't be read.
pub(crate) fn last_run_text(task: &dyn DenyTask) -> String {
    match task.last_run() {
        Err(why) => format!("unreadable ({why})"),
        Ok(LastRun::NeverRun) => "never run".to_owned(),
        Ok(LastRun::Ran { code, .. }) if code == SCHED_S_TASK_RUNNING => "running".to_owned(),
        Ok(LastRun::Ran { code, .. }) => match WslDenyExit::from_code(code) {
            Some(exit) => format!("{exit:?} ({code})"),
            None => format!("code {code:#010x}"),
        },
    }
}

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;
