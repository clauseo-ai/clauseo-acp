//! The outer runner's start of the inner runner, in `start.rs`'s order: sweep (a), the logon,
//! the token user's check, the job, the shut, sweep (b), the resume.

use zeroize::Zeroizing;

use super::super::RunExit;
use super::super::spawn::{Process, assign, log_on, resume};
use super::super::start::Steps;
use super::super::sweep::os::OsProcesses;
use super::super::sweep::{Point, sweep};
use super::{Failure, logon_failure, os, say};
use crate::windows::acl::object::Handle;
use crate::windows::sids::Sid;
use crate::windows::token::os::shut_object;
use crate::windows::token::{runner_process_sddl, runner_thread_sddl};

/// What the start needs; the password is spent by the logon.
pub(super) struct Start<'a> {
    pub(super) pool_name: &'a str,
    pub(super) pool_user: &'a Sid,
    pub(super) desktop: &'a Sid,
    pub(super) password: Option<Zeroizing<String>>,
    pub(super) exe: &'a str,
    pub(super) base: &'a str,
    pub(super) cwd: &'a str,
    pub(super) job: &'a Handle,
}

/// Sweeps `pool_user`'s processes at `point`, keeping the pids in `keep`; each kill is a line on
/// stderr.
pub(super) fn sweep_at(
    job: &Handle,
    pool_user: &Sid,
    keep: &[u32],
    point: Point,
) -> Result<(), String> {
    let mut calls = OsProcesses { job };
    let mut log = |line: &str| say(line);
    sweep(&mut calls, pool_user, keep, point, &mut log)
        .map(drop)
        .map_err(|error| format!("sweep ({point}): {error}"))
}

impl Steps for Start<'_> {
    type Inner = Process;
    type Error = Failure;

    fn sweep(&mut self, point: Point, inner: Option<&Process>) -> Result<(), Failure> {
        let keep: Vec<u32> = inner.map(|inner| inner.pid).into_iter().collect();
        sweep_at(self.job, self.pool_user, &keep, point)
            .map_err(|what| Failure::new(RunExit::Sweep, what))
    }

    fn log_on(&mut self) -> Result<Process, Failure> {
        let password = self
            .password
            .take()
            .ok_or_else(|| Failure::new(RunExit::Logon, "the password was spent"))?;
        // `log_on` takes the password and wipes it as it returns.
        log_on(self.pool_name, password, self.exe, self.base, self.cwd).map_err(logon_failure)
    }

    /// Its token's user, read now: from its first instruction the inner shuts its token to all
    /// but SYSTEM until the launch arrives over `ctl`.
    fn check_user(&mut self, inner: &Process) -> Result<(), Failure> {
        let user = inner
            .user()
            .map_err(|e| Failure::new(RunExit::Logon, os("the inner runner's token", &e)))?;
        if user != *self.pool_user {
            return Err(Failure::new(
                RunExit::Logon,
                "the inner runner isn't the pool user",
            ));
        }
        Ok(())
    }

    fn assign(&mut self, inner: &Process) -> Result<(), Failure> {
        assign(self.job, inner)
            .map_err(|e| Failure::new(RunExit::Job, os("AssignProcessToJobObject", &e)))
    }

    /// Shuts the inner runner's process and first thread to its agent (the same pool user)
    /// while it is still suspended; sweep (b) then kills any process of the pool user that
    /// opened either, or its token, before this.
    fn shut(&mut self, inner: &Process) -> Result<(), Failure> {
        shut_object(inner.handle.0, &runner_process_sddl(self.desktop))
            .and_then(|()| shut_object(inner.thread.0, &runner_thread_sddl(self.desktop)))
            .map_err(|e| Failure::new(RunExit::Token, os("the inner runner's DACL", &e)))
    }

    fn resume(&mut self, inner: &Process) -> Result<(), Failure> {
        resume(inner).map_err(|e| Failure::new(RunExit::Job, os("ResumeThread", &e)))
    }

    fn kill(&mut self, inner: &Process, error: &Failure) {
        inner.kill(error.0.code());
    }
}
