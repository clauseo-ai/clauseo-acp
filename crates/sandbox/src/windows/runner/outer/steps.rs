//! The outer runner's start of the inner runner, in `start.rs`'s order: the host's denial pipe
//! connected, the git watch armed, a previous session's runners waited for and ended, the logon,
//! the token user's check, the job, the shut, the resume (none of it once the session's end has
//! begun), and the full check before the go. The inner gets no handle to the job.

use std::sync::Arc;

use zeroize::Zeroizing;

use super::super::end::ended_before_start;
use super::super::frame::{InnerGo, write_frame};
use super::super::inner_user::{READY_WAIT, TokenSeen, check_full, check_inner_user, ready_within};
use super::super::previous::clear;
use super::super::spawn::{Process, assign, log_on, resume, telemetry_user};
use super::super::start::Steps;
use super::super::station::StationDesktop;
use super::super::sweep::os::OsPrevious;
use super::super::{LaunchMessage, RunExit};
use super::{End, FIRST_CONNECT, Failure, accept, inner_exit, logon_failure, os, watch};
use crate::windows::acl::object::Handle;
use crate::windows::denial_pipe::os::open_writer;
use crate::windows::denial_pipe::sink::{RecordSink, open_feed};
use crate::windows::pipe::os::{ServerPipe, Until};
use crate::windows::sids::Sid;
use crate::windows::token::os::{shut_object, shut_process};
use crate::windows::token::{runner_process_sddl, runner_thread_sddl};

/// What the start needs; the password is spent by the logon.
pub(super) struct Start<'a> {
    pub(super) pool_name: &'a str,
    pub(super) pool_user: &'a Sid,
    pub(super) desktop: &'a Sid,
    /// The desktop user's logon SID, which the inner's token must not hold (`check_full`).
    pub(super) logon: &'a Sid,
    pub(super) password: Option<Zeroizing<String>>,
    pub(super) exe: &'a str,
    pub(super) base: &'a str,
    pub(super) cwd: &'a str,
    /// The agent's station's desktop, the inner runner's too.
    pub(super) station_desktop: &'a StationDesktop,
    pub(super) job: &'a Handle,
    /// The `ctl` pipe, on which the inner says it is ready and is told to go.
    pub(super) ctl: &'a ServerPipe,
    /// The session's end, which the start checks (`start.rs`).
    pub(super) end: &'a End,
    /// The launch: its denial pipe and the folders the watch covers.
    pub(super) message: &'a LaunchMessage,
    /// The denial pipe's sink, once connected.
    pub(super) records: Option<Arc<RecordSink>>,
}

impl Steps for Start<'_> {
    type Inner = Process;
    type Error = Failure;

    /// A previous session's live runners of the pool user (an inner still running) are waited
    /// for, then ended, before the logon (`previous.rs`).
    fn clear(&mut self) -> Result<(), Failure> {
        let image = std::path::Path::new(self.exe)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Failure::new(RunExit::Usage, "the runner's file name"))?;
        let mut calls = OsPrevious {
            pool_user: self.pool_user,
            image,
        };
        clear(&mut calls).map_err(|(exit, message)| Failure::new(exit, message))
    }

    /// Takes the denial pipe's one instance, before anything runs as the pool user. A pipe the
    /// launch names that can't be opened refuses the launch, in plain words.
    fn feed(&mut self) -> Result<(), Failure> {
        let sink = open_feed(self.message.denial_pipe.as_deref(), |name| {
            open_writer(name)
        })
        .map_err(|why| Failure::new(RunExit::Pipe, why))?;
        self.records = Some(Arc::new(sink));
        Ok(())
    }

    /// The git watch on the matter's folder, armed before the agent runs. It can end the
    /// session from here: then the start makes no inner, or kills it unresumed.
    fn watch(&mut self) -> Result<(), Failure> {
        let records = self
            .records
            .clone()
            .unwrap_or_else(|| Arc::new(RecordSink::start(None)));
        watch::keep(self.message, self.end, records)
    }

    fn log_on(&mut self) -> Result<Process, Failure> {
        let password = self
            .password
            .take()
            .ok_or_else(|| Failure::new(RunExit::Logon, "the password was spent"))?;
        // `log_on` takes the password and wipes it as it returns.
        // The desktop user's SID, so the inner's first act can grant it `TOKEN_QUERY` on its
        // token (`token::runner_token_first_sddl`) before the launch names it.
        let args = format!("--inner {} {}", self.base, self.desktop);
        log_on(
            self.pool_name,
            password,
            self.exe,
            &args,
            self.cwd,
            self.station_desktop,
        )
        .map_err(logon_failure)
    }

    /// Its token's user, read now, while it is suspended (the first stage, `start.rs`). On the
    /// agent's own station its token refuses this process (5) until its first act grants it
    /// `TOKEN_QUERY`, and the user is read without the token (`inner_user.rs`).
    fn check_user(&mut self, inner: &Process) -> Result<(), Failure> {
        check_inner_user(self.pool_user, inner.user(), || {
            telemetry_user(inner.handle.0, inner.pid)
        })
        .map_err(|(exit, message)| Failure::new(exit, message))
    }

    fn assign(&mut self, inner: &Process) -> Result<(), Failure> {
        assign(self.job, inner.handle.0)
            .map_err(|e| Failure::new(RunExit::Job, os("AssignProcessToJobObject", &e)))
    }

    /// Shuts the inner runner's process and first thread to its agent (the same pool user)
    /// while it is still suspended; its first act once resumed kills any process of the pool
    /// user's that opened either, or its token, before this.
    fn shut(&mut self, inner: &Process) -> Result<(), Failure> {
        shut_process(inner.handle.0, &runner_process_sddl(self.desktop))
            .and_then(|()| shut_object(inner.thread.0, &runner_thread_sddl(self.desktop)))
            .map_err(|e| Failure::new(RunExit::Token, os("the inner runner's DACL", &e)))
    }

    fn resume(&mut self, inner: &Process) -> Result<(), Failure> {
        resume(inner).map_err(|e| Failure::new(RunExit::Job, os("ResumeThread", &e)))
    }

    /// The second stage of the check (`start.rs`): the inner's ready on `ctl`, once its token
    /// grants this user `TOKEN_QUERY`, then its token (`inner_user::check_full`), then the
    /// go. An inner that exited first (a failed grant: `RunExit::TokenGrant`) says why in its
    /// exit code.
    fn confirm(&mut self, inner: &Process) -> Result<(), Failure> {
        if let Err(failure) = accept(self.ctl, inner, FIRST_CONNECT) {
            return Err(inner_exit(inner).unwrap_or(failure));
        }
        // Bounded (`READY_WAIT`): an inner that never says ready is killed (`start.rs`).
        let ready = ready_within(Until::new(self.ctl, READY_WAIT)).map_err(|(exit, message)| {
            inner_exit(inner).unwrap_or_else(|| Failure::new(exit, message))
        })?;
        let token = inner.token_parts().map_err(|e| {
            Failure::new(
                RunExit::Logon,
                os("the inner runner's full check: its token", &e),
            )
        })?;
        let seen = TokenSeen {
            ready_pid: ready.pid,
            token,
        };
        check_full(&seen, inner.pid, self.pool_user, self.logon)
            .map_err(|(exit, message)| Failure::new(exit, message))?;
        write_frame(self.ctl, &InnerGo { go: true })
            .map_err(|e| Failure::new(RunExit::Pipe, os("ctl write", &e)))
    }

    fn kill(&mut self, inner: &Process, error: &Failure) {
        inner.kill(error.0.code());
    }

    /// A session the git watch ended before its agent ran exits as the job's end would have
    /// ended a running one (`end::ended_before_start`).
    fn not_ended(&mut self) -> Result<(), Failure> {
        let Some(began) = self.end.began() else {
            return Ok(());
        };
        let exit = RunExit::from_code(ended_before_start(began)).unwrap_or(RunExit::AgentReserved);
        Err(Failure::new(
            exit,
            "the session ended before its agent started; the agent never ran",
        ))
    }
}
