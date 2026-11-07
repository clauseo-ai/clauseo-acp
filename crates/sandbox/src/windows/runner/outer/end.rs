//! The detached end sweeper's calls (Windows), for `end.rs`'s order: the launch's job ended
//! (`TerminateJobObject`) and waited for until `ActiveProcesses` is 0, then `--sweep` started as
//! the pool user with `CreateProcessWithLogonW`, suspended, its process and thread shut as the
//! inner's are, then resumed. It is never assigned to this launch's job: the Secondary Logon
//! service starts it as its own child, outside the outer's jobs, in a job of the service's own,
//! so the host ending the outer's job leaves it running. Its stdout and stderr are a pipe whose
//! lines this runner writes to its own stderr while it lives; on a cancel nobody reads them.

use std::io::{BufRead as _, BufReader};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use super::super::RunExit;
use super::super::end::{EMPTY_POLL, EMPTY_WAIT, SweeperSteps, wait_empty};
use super::super::spawn::{Job, Process, active_processes, end_job, log_on, output_pipe, resume};
use super::super::sweep::SWEPT;
use super::{os, say};
use crate::windows::acl::object::Handle;
use crate::windows::secrets::secret_for;
use crate::windows::sids::Sid;
use crate::windows::token::os::shut_object;
use crate::windows::token::{runner_process_sddl, runner_thread_sddl};

/// How long the outer, still alive at its end, waits for the sweeper to report.
const SWEEP_WAIT: Duration = Duration::from_secs(20);

/// What the sweeper's start needs.
pub(super) struct OsSweeper {
    pub(super) data_dir: PathBuf,
    pub(super) pool_user: Sid,
    pub(super) pool_name: String,
    pub(super) desktop: Sid,
    /// The desktop user's logon SID, which the runners' process DACL grants.
    pub(super) logon: Sid,
    pub(super) exe: String,
    pub(super) cwd: String,
    /// The sweeper once running.
    pub(super) kept: Mutex<Option<Process>>,
    /// The launch's kill-on-close job, once `session` made it.
    pub(super) job: OnceLock<Job>,
}

impl OsSweeper {
    /// Keeps `job`, made once the launch is read, for [`SweeperSteps::empty_job`]; the handle the
    /// session uses. It closes when this process exits.
    pub(super) fn hold_job(&self, job: Handle) -> &Handle {
        &self.job.get_or_init(move || Job(job)).0
    }

    /// At the outer's end, if it lives that long: waits up to [`SWEEP_WAIT`] for the sweeper and
    /// logs a failure or a sweeper still running.
    pub(super) fn wait(&self) {
        let Ok(mut kept) = self.kept.lock() else {
            return;
        };
        let Some(sweeper) = kept.take() else {
            return;
        };
        match sweeper.wait(Some(SWEEP_WAIT)) {
            Ok(Some(0)) => {}
            Ok(Some(code)) => say(&format!(
                "clauseo-sandbox-run: the end sweeper failed (0x{code:08X})"
            )),
            Ok(None) => say("clauseo-sandbox-run: the end sweeper is still running"),
            Err(error) => say(&format!(
                "clauseo-sandbox-run: {}",
                os("the end sweeper's wait", &error)
            )),
        }
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

impl SweeperSteps for OsSweeper {
    type Sweeper = Process;

    fn empty_job(&self) -> Result<(), String> {
        let Some(Job(job)) = self.job.get() else {
            return Ok(());
        };
        end_job(job, SWEPT);
        wait_empty(|| active_processes(job), pause, EMPTY_WAIT, EMPTY_POLL)
    }

    fn log_on(&self) -> Result<Process, String> {
        let password = secret_for(&self.data_dir, &self.pool_user).map_err(|e| e.to_string())?;
        let (read, write) = output_pipe().map_err(|e| os("CreatePipe", &e))?;
        // `log_on` takes the password and wipes it as it returns.
        let sweeper = log_on(
            &self.pool_name,
            password,
            &self.exe,
            "--sweep",
            &self.cwd,
            Some(write.0),
        )
        .map_err(|code| format!("CreateProcessWithLogonW failed ({code})"))?;
        // Only the sweeper's copy of the write end is left, so its exit ends the reads.
        drop(write);
        let relay = thread::Builder::new().spawn(move || {
            for line in BufReader::new(read).lines() {
                match line {
                    Ok(line) => say(&line),
                    Err(_) => return,
                }
            }
        });
        if let Err(error) = relay {
            say(&format!(
                "clauseo-sandbox-run: {}",
                os("the end sweeper's log", &error)
            ));
        }
        Ok(sweeper)
    }

    fn shut(&self, sweeper: &Process) -> Result<(), String> {
        shut_object(
            sweeper.handle.0,
            &runner_process_sddl(&self.desktop, &self.logon),
        )
        .and_then(|()| shut_object(sweeper.thread.0, &runner_thread_sddl(&self.desktop)))
        .map_err(|e| os("the end sweeper's DACL", &e))
    }

    fn resume(&self, sweeper: &Process) -> Result<(), String> {
        resume(sweeper).map_err(|e| os("ResumeThread", &e))
    }

    fn kill(&self, sweeper: &Process) {
        sweeper.kill(RunExit::Sweep.code());
    }

    fn keep(&self, sweeper: Process) {
        if let Ok(mut kept) = self.kept.lock() {
            *kept = Some(sweeper);
        }
    }

    fn log(&self, line: &str) {
        say(line);
    }
}
