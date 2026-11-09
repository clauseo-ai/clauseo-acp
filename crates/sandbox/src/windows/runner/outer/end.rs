//! The session end's calls (Windows), for `end.rs`'s order: the launch's job ended
//! (`TerminateJobObject`) and waited for until `ActiveProcesses` is 0, the record's `SweepNext`
//! and `StopNext` read as the desktop user (the record grants Users read), runs of the `SYSTEM`
//! tasks `\Clauseo\SandboxSweep` and `\Clauseo\SandboxStop` requested as the desktop user
//! (`refresh::request_pool_sweep`, `refresh::request_pool_stop`), and the session's marker
//! written. A run is the Task Scheduler's once queued: the host ending the outer's job leaves it
//! running. No process of the pool user's is started here.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use super::super::end::{EMPTY_POLL, EMPTY_WAIT, EndSteps, wait_empty};
use super::super::marker::{MarkerState, write_marker_at};
use super::super::spawn::{Job, active_processes, end_job};
use super::super::sweep::SWEPT;
use super::say;
use crate::windows::acl::object::Handle;
use crate::windows::refresh::{request_pool_stop, request_pool_sweep};
use crate::windows::setup::{read_record_install_id, read_record_value};
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
    /// The session's pool user, whom the marker names.
    pub(super) pool_user: Sid,
}

impl OsEnd {
    /// Keeps `job`, made once the launch is read, for [`EndSteps::empty_job`]; the handle the
    /// session uses. It closes when this process exits.
    pub(super) fn hold_job(&self, job: Handle) -> &Handle {
        &self.job.get_or_init(move || Job(job)).0
    }
}

/// The pause between two looks at the ended job's process count.
#[expect(
    clippy::disallowed_methods,
    reason = "the outer's bounded wait for its ended job to empty, 25 ms at a time; the tests \
              inject the pause (`end::wait_empty`)"
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

/// The monotonic time since this process's first reading: [`EndSteps::now`], and the
/// policy cache's clock (`steps::logon_failure`).
pub(super) fn since_start() -> Duration {
    let origin = *ORIGIN.get_or_init(clock);
    clock().saturating_duration_since(origin)
}

/// [`EndSteps::now`]'s origin: its first reading.
static ORIGIN: OnceLock<Instant> = OnceLock::new();

impl EndSteps for OsEnd {
    fn empty_job(&self) -> Result<(), String> {
        let Some(Job(job)) = self.job.get() else {
            return Ok(());
        };
        end_job(job, SWEPT);
        wait_empty(|| active_processes(job), pause, EMPTY_WAIT, EMPTY_POLL)
    }

    fn next_number(&self, name: &'static str) -> Result<u64, String> {
        let read = read_record_value(name).map_err(|error| error.to_string())?;
        Ok(read.unwrap_or(1))
    }

    fn request_sweep(&self) -> Result<(), String> {
        request_pool_sweep().map_err(|error| error.to_string())
    }

    fn request_stop(&self) -> bool {
        request_pool_stop()
    }

    fn settle(&self, state: &MarkerState) -> Result<(), String> {
        // The install's id, read again: no removal or reinstall runs while a session is open. One
        // that can't be read leaves the marker open, which never passes for clean.
        let install = read_record_install_id().map_err(|error| error.to_string())?;
        let at = install.as_deref();
        write_marker_at(&self.marker, Some(&self.pool_user), at, *state).map_err(|error| {
            error
                .raw_os_error()
                .map_or_else(|| error.kind().to_string(), |code| code.to_string())
        })
    }

    fn log(&self, line: &str) {
        say(line);
    }

    fn now(&self) -> Duration {
        since_start()
    }
}
