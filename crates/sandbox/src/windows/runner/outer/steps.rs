//! The outer runner's start of the inner runner, in `start.rs`'s order: the logon, the token
//! user's check, the job, the shut, the resume. The inner gets no handle to the job.

use zeroize::Zeroizing;

use super::super::spawn::{Process, assign, log_on, resume};
use super::super::start::Steps;
use super::super::{RunExit, logon_refusal, recorded_state};
use super::end::since_start;
use super::{Failure, os};
use crate::windows::acl::object::Handle;
use crate::windows::record::{sandbox_group_sid, sandbox_logon_policy};
use crate::windows::setup::rights_policy::{INTERACTIVE_RIGHT, Pool, RightPolicy};
use crate::windows::setup::rights_source;
use crate::windows::sids::Sid;
use crate::windows::token::os::shut_object;
use crate::windows::token::{runner_process_sddl, runner_thread_sddl};

/// What the start needs; the password is spent by the logon.
pub(super) struct Start<'a> {
    pub(super) pool_name: &'a str,
    pub(super) pool_user: &'a Sid,
    pub(super) desktop: &'a Sid,
    /// The desktop user's logon SID, which the runners' process DACL grants.
    pub(super) logon: &'a Sid,
    pub(super) password: Option<Zeroizing<String>>,
    pub(super) exe: &'a str,
    pub(super) base: &'a str,
    pub(super) cwd: &'a str,
    pub(super) job: &'a Handle,
}

impl Steps for Start<'_> {
    type Inner = Process;
    type Error = Failure;

    fn log_on(&mut self) -> Result<Process, Failure> {
        let password = self
            .password
            .take()
            .ok_or_else(|| Failure::new(RunExit::Logon, "the password was spent"))?;
        // `log_on` takes the password and wipes it as it returns.
        let args = format!("--inner {}", self.base);
        log_on(self.pool_name, password, self.exe, &args, self.cwd)
            .map_err(|code| logon_failure(code, self.pool_user))
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
        assign(self.job, inner.handle.0)
            .map_err(|e| Failure::new(RunExit::Job, os("AssignProcessToJobObject", &e)))
    }

    /// Shuts the inner runner's process and first thread to its agent (the same pool user)
    /// while it is still suspended; its first act once resumed kills any process of the pool
    /// user's that opened either, or its token, before this.
    fn shut(&mut self, inner: &Process) -> Result<(), Failure> {
        shut_object(
            inner.handle.0,
            &runner_process_sddl(self.desktop, self.logon),
        )
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

/// Maps a `CreateProcessWithLogonW` code to its failure. A 1385 reads the record's
/// `LogonPolicy`, then the cached policy read, to learn whether a policy leaves the pool out of
/// "Allow log on locally" ([`logon_refusal`]), so the words say whether setting up again helps;
/// read only on a 1385, never on a launch that works.
pub(super) fn logon_failure(code: u32, pool_user: &Sid) -> Failure {
    match code {
        1058 => Failure::new(RunExit::SecondaryLogon, super::super::SECLOGON_DISABLED),
        1385 => {
            let recorded = recorded_state(sandbox_logon_policy());
            let (exit, blocked) = logon_refusal(recorded, || launch_policy(pool_user));
            Failure::new(exit, blocked.to_string())
        }
        1326 => Failure::new(RunExit::Password, super::super::PASSWORD_LOST),
        other => Failure::new(
            RunExit::Logon,
            format!("CreateProcessWithLogonW failed ({other})"),
        ),
    }
}

/// What the cached policy read (`rights_source::cached`) says of "Allow log on locally" for the
/// record's group and `pool_user`. As the desktop user the template cache may not be readable;
/// that is [`RightPolicy::Unknown`], which keeps the words of a local 1385.
fn launch_policy(pool_user: &Sid) -> RightPolicy {
    let group = sandbox_group_sid()
        .ok()
        .flatten()
        .and_then(|text| Sid::parse(&text).ok());
    let pool = Pool {
        group: group.as_ref(),
        users: std::slice::from_ref(pool_user),
    };
    rights_source::cached(INTERACTIVE_RIGHT, pool, since_start())
}
