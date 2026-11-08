//! The session end's calls (Windows), for `end.rs`'s order: the launch's job ended
//! (`TerminateJobObject`) and waited for until `ActiveProcesses` is 0, then a run of the `SYSTEM`
//! task `\Clauseo\SandboxSweep` requested as the desktop user (`refresh::request_pool_sweep`) and,
//! at the outer's end, its result read (`refresh::pool_sweep_result`). The run is the Task
//! Scheduler's once queued: the host ending the outer's job leaves it running. No process of the
//! pool user's is started here.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use super::super::end::{EMPTY_POLL, EMPTY_WAIT, EndSteps, wait_empty};
use super::super::spawn::{Job, active_processes, end_job};
use super::super::sweep::SWEPT;
use super::say;
use crate::windows::acl::object::Handle;
use crate::windows::refresh::{SweepRun, pool_sweep_result, request_pool_sweep};
use crate::windows::sids::Sid;

/// What the session and its end need of the outer's own.
pub(super) struct OsEnd {
    /// The data folder, whose `secrets.bin` holds the pool user's password.
    pub(super) data_dir: PathBuf,
    /// The desktop user.
    pub(super) desktop: Sid,
    /// The desktop user's logon SID, which the runners' process DACL grants.
    pub(super) logon: Sid,
    /// This program's own path, which the inner runner runs.
    pub(super) exe: String,
    /// System32, the inner runner's current folder.
    pub(super) cwd: String,
    /// The launch's kill-on-close job, once `session` made it.
    pub(super) job: OnceLock<Job>,
    /// The session's unclean-session marker (`marker.rs`).
    pub(super) marker: PathBuf,
}

impl OsEnd {
    /// Keeps `job`, made once the launch is read, for [`EndSteps::empty_job`]; the handle the
    /// session uses. It closes when this process exits.
    pub(super) fn hold_job(&self, job: Handle) -> &Handle {
        &self.job.get_or_init(move || Job(job)).0
    }
}

/// The pause between two looks at the ended job's process count or the sweep task's result.
#[expect(
    clippy::disallowed_methods,
    reason = "the outer's bounded waits for its ended job to empty, 25 ms at a time, and for the \
              sweep task's result, 100 ms at a time; the tests inject the pause \
              (`end::wait_empty`, `EndSteps::pause`)"
)]
fn pause(poll: Duration) {
    thread::sleep(poll);
}

/// The monotonic clock's reading, for [`EndSteps::now`].
#[expect(
    clippy::disallowed_methods,
    reason = "the outer measures how much of the agent's grace after the host's stdin end is \
              left; the tests inject the clock (`EndSteps::now`)"
)]
fn clock() -> Instant {
    Instant::now()
}

/// [`EndSteps::now`]'s origin: its first reading.
static ORIGIN: OnceLock<Instant> = OnceLock::new();

impl EndSteps for OsEnd {
    type Run = SweepRun;

    fn empty_job(&self) -> Result<(), String> {
        let Some(Job(job)) = self.job.get() else {
            return Ok(());
        };
        end_job(job, SWEPT);
        wait_empty(|| active_processes(job), pause, EMPTY_WAIT, EMPTY_POLL)
    }

    fn request_sweep(&self) -> Result<SweepRun, String> {
        request_pool_sweep().map_err(|error| error.to_string())
    }

    fn sweep_result(&self, run: &SweepRun) -> Result<Option<u32>, String> {
        pool_sweep_result(run).map_err(|error| error.to_string())
    }

    fn pause(&self, pause: Duration) {
        self::pause(pause);
    }

    fn log(&self, line: &str) {
        say(line);
    }

    fn now(&self) -> Duration {
        let origin = *ORIGIN.get_or_init(clock);
        clock().saturating_duration_since(origin)
    }
}
