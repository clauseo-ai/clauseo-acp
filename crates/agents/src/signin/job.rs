//! The job model of every sign-in, sign-out and status check: one job per agent at a time, each
//! on its own thread, following the install jobs' pattern (`install.rs`, `worker.rs`).
//!
//! - A call on `Agents` checks what it can at once (the agent, its method, its home, its
//!   install) and reserves the agent's job slot under the shared lock: a new [`Ticket`], and
//!   for a sign-in the channel pasted codes go down. A sign-in or sign-out supersedes the
//!   agent's running job; a status check never does. The worker thread then starts the job.
//! - The job hands its [`Task`] to the agent's flow ([`super::claude`]; Codex's in piece 4).
//!   Every event goes through `SigninHub::emit_job`, which drops it once the job is no
//!   longer the agent's: a superseded job is silent, and its child is killed by its
//!   [`Ticket::cancel`].
//! - `cancel_sign_in` sets the ticket's cancel but keeps the job the agent's, so the job itself
//!   reports `Failed { code: Cancelled }`, after its child is killed and its URL file removed.
//! - The flow's last state ends the job; `None` ends it with the state unchanged.
//!
//! Detection and reuse (piece 5) add tasks here; the queued start (piece 6) reserves the slot
//! early and starts the same job once the install is ready and the network is up.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use clauseo_process::{Launcher, Timer};

use crate::config::AgentsConfig;
use crate::signin::env::{EnvInputs, RunKind, Shim, Vendor, run_env};
use crate::signin::run::RunSpec;
use crate::signin::{Method, PasteCode, SigninCode, SigninState, claude};
use crate::state::Installed;
use crate::worker::{Shared, Ticket};

/// What a job does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Task {
    /// A login run with this method, watched until it is confirmed, fails, expires or is
    /// cancelled.
    SignIn(Method),
    /// The vendor's logout in Clauseo's own folder, then a status check.
    SignOut,
    /// One status check (R20's "a window shows Plans").
    Status,
}

impl Task {
    /// Whether this task replaces the agent's running job (a status check never does).
    pub(crate) fn supersedes(self) -> bool {
        !matches!(self, Task::Status)
    }
}

/// Everything one job needs, moved onto its thread.
pub(crate) struct SigninJob<T> {
    pub(crate) ticket: Ticket,
    pub(crate) task: Task,
    pub(crate) vendor: Vendor,
    pub(crate) config: AgentsConfig,
    /// The agent's install: `node` runs `script`.
    pub(crate) installed: Installed,
    /// The agent's home, `SigninConfig::homes`' text.
    pub(crate) home: PathBuf,
    /// The shim's path, when the worker wrote it (macOS and Linux sign-ins).
    pub(crate) browser: Option<PathBuf>,
    pub(crate) launcher: Arc<dyn Launcher>,
    pub(crate) timer: Arc<T>,
    pub(crate) shared: Arc<Shared>,
    /// Pasted codes, for a sign-in.
    pub(crate) paste: Option<async_channel::Receiver<PasteCode>>,
}

/// The job thread's body: the flow, then its last state.
pub(crate) fn run<T: Timer + Send + Sync>(mut job: SigninJob<T>) {
    let paste = job.paste.take();
    let last = futures::executor::block_on(job.work(paste));
    match last {
        Some(state) => {
            job.shared.signin.emit_job(&job.ticket, state, true);
        }
        None => job.shared.signin.end_job(&job.ticket),
    }
}

impl<T: Timer + Send + Sync> SigninJob<T> {
    async fn work(&self, paste: Option<async_channel::Receiver<PasteCode>>) -> Option<SigninState> {
        if !self.shared.signin.is_job(&self.ticket) {
            return None;
        }
        match (self.vendor, self.task) {
            (Vendor::Claude, Task::SignIn(method)) => {
                Some(claude::sign_in(self, method == Method::Console, paste).await)
            }
            (Vendor::Claude, Task::SignOut) => claude::sign_out(self).await,
            (Vendor::Claude, Task::Status) => claude::check_status(self).await,
            // S18 piece 4: the calls refuse Codex before a job starts.
            (Vendor::Codex, _) => None,
        }
    }

    /// The clock's now: `since` and `at`.
    pub(crate) fn now(&self) -> SystemTime {
        self.config.clock.now()
    }

    /// `Failed { code, at: now }`.
    pub(crate) fn failed(&self, code: SigninCode) -> SigninState {
        SigninState::Failed {
            code,
            at: self.now(),
        }
    }

    /// Reports `state` while the job goes on; nothing once it is superseded.
    pub(crate) fn report(&self, state: SigninState) {
        self.shared.signin.emit_job(&self.ticket, state, false);
    }

    /// Sends `OpenUrl` for `url`, already checked against [`SigninJob::hosts`].
    pub(crate) fn open_url(&self, url: String) {
        self.shared.signin.emit_open_url(&self.ticket, url);
    }

    /// The snapshot's sign-in hosts for this agent.
    pub(crate) fn hosts(&self) -> &[String] {
        self.config
            .snapshot
            .agent(&self.ticket.agent)
            .map(|entry| entry.signin_hosts.as_slice())
            .unwrap_or_default()
    }

    /// The child's whole environment for a run of `kind`, with `shim` for a login.
    pub(crate) fn env(
        &self,
        kind: RunKind<'_>,
        shim: Option<Shim<'_>>,
    ) -> BTreeMap<OsString, OsString> {
        run_env(&EnvInputs {
            base: &self.config.signin.base_env,
            vendor: self.vendor,
            kind,
            os: self.config.platform.os,
            proxy: self.config.install.proxy.as_deref(),
            ca_file: self
                .config
                .install
                .ca_certs
                .get(&self.ticket.agent)
                .map(PathBuf::as_path),
            shim,
        })
    }

    /// `<node> <script> args…` in the agent's home, with `env`, killed past `limit`.
    pub(crate) fn spec(
        &self,
        args: &[&str],
        env: BTreeMap<OsString, OsString>,
        limit: Duration,
    ) -> RunSpec {
        RunSpec {
            node: self.installed.node.clone(),
            script: self.installed.script.clone(),
            args: args.iter().map(OsString::from).collect(),
            env,
            cwd: self.home.clone(),
            limit,
        }
    }
}
