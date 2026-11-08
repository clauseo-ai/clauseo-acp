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

use std::time::Duration;

use crate::Blocked;
use crate::windows::refresh::WslDenyExit;
use crate::windows::refresh::last_run::{LastRun, SCHED_S_TASK_RUNNING};
use crate::windows::wsl::Found;

/// How often the check is made again while the task runs.
pub const POLL: Duration = Duration::from_millis(250);
/// How long the launch waits for a missing deny: the task's run takes a moment.
pub const DENY_WAIT: Duration = Duration::from_secs(10);

/// What the self-healing did, for the host's log ([`WslHealed::log_line`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WslHealed {
    /// Nothing was owed: the deny was in place, or there is no WSL.
    #[default]
    Nothing,
    /// The deny was missing, the WSL deny task put it back, and the launch went on.
    DenyRestored,
}

impl WslHealed {
    /// The line the host logs for it (host.log), or `None` when nothing was done.
    pub fn log_line(self) -> Option<&'static str> {
        match self {
            WslHealed::Nothing => None,
            WslHealed::DenyRestored => {
                Some("WSL's deny for the sandbox users was missing; the WSL deny task put it back")
            }
        }
    }
}

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

/// The WSL check (`check`, `wsl::check` on the machine), healed as the module says: what it
/// found and what the healing did, or the refusal and why.
pub(crate) fn heal(
    check: &mut dyn FnMut() -> Result<Found, Blocked>,
    task: &dyn DenyTask,
) -> Result<(Found, WslHealed), Refused> {
    let mut now = check();
    match now {
        Ok(found) => return Ok((found, WslHealed::Nothing)),
        Err(Blocked::WslDenyMissing) => {}
        Err(blocked) => {
            let why = "not something the WSL deny task repairs".to_owned();
            return Err(Refused { blocked, why });
        }
    }
    if let Err(why) = task.request() {
        let why = format!("the WSL deny task couldn't be started: {why}");
        return Err(refused(now, &why, task));
    }
    let start = task.elapsed();
    while task.elapsed().saturating_sub(start) < DENY_WAIT {
        task.sleep(POLL);
        now = check();
        if let Ok(found) = now {
            return Ok((found, WslHealed::DenyRestored));
        }
    }
    let secs = DENY_WAIT.as_secs();
    Err(refused(now, &format!("still so after {secs} s"), task))
}

/// The refusal `now` gives, `why` followed by the task's last result.
fn refused(now: Result<Found, Blocked>, why: &str, task: &dyn DenyTask) -> Refused {
    let blocked = now.err().unwrap_or(Blocked::WslDenyMissing);
    let last = last_run_text(task);
    let why = format!("{why}; the WSL deny task's last run: {last}");
    Refused { blocked, why }
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
