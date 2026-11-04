//! The job model of every sign-in, sign-out and status check: one job per agent at a time, each
//! on its own thread, following the install jobs' pattern (`install.rs`, `worker.rs`).
//!
//! - A call on `Agents` checks what it can at once (the agent, its method, its home, its
//!   install) and reserves the agent's job slot under the shared lock: a new [`Ticket`], and
//!   for a sign-in the channel pasted codes go down. A sign-in or sign-out supersedes the
//!   agent's running job; a status check never does. The worker thread then starts the job.
//! - The job hands its [`Task`] to the agent's flow ([`super::claude`] or [`super::codex`]).
//!   Every event goes through `SigninHub::emit_job`, which drops it once the job is no
//!   longer the agent's: a superseded job is silent, and its child is killed by its
//!   [`Ticket::cancel`].
//! - `cancel_sign_in` sets the ticket's cancel but keeps the job the agent's, so the job itself
//!   reports `Failed { code: Cancelled }`, after its child is killed and its URL file removed.
//! - The flow's last state ends the job; `None` ends it with the state unchanged.
//!
//! Reuse (piece 5) adds the lawyer's choices about a login they already have as tasks
//! ([`super::found`]), and the status check becomes the start check: reuse first, then
//! Clauseo's own login, then detection. A sign-in of Clauseo's own that ends signed in records
//! `login-source` `own`.
//! - A task that starts a login run ([`Task::queues`]) first waits in [`super::queue`] until
//!   the install is `Ready` and the sign-in host resolves, reporting `Queued` meanwhile; the
//!   call took the slot at once, so a cancel or a new sign-in reaches the queued job as it
//!   reaches a running one.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use clauseo_process::{Launcher, Timer};

use crate::config::AgentsConfig;
use crate::signin::env::{EnvInputs, RunKind, Shim, Vendor, run_env};
use crate::signin::marker::{self, LoginSource};
use crate::signin::queue::{self, Gate, Queue};
use crate::signin::run::RunSpec;
use crate::signin::{
    Found, Method, PasteCode, SigninCode, SigninNote, SigninState, Source, claude, codex, found,
};
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
    /// Codex's "Try again" after `signin.port_busy`: port 1455 is probed every 2 s, and the
    /// browser sign-in starts once it is free.
    TryAgain,
    /// "Look for my … login": `no-reuse` goes, then the status check.
    FindLogin,
    /// "Use it": reuse the login `LoginFound` showed.
    UseFound,
    /// "Sign in with a different account": `no-reuse`, then Clauseo's own sign-in.
    UseDifferent,
    /// "Stop using it here" (no reason) or reuse stopping for a reason: no logout.
    StopUsing(Option<Found>),
}

impl Task {
    /// Whether this task replaces the agent's running job (a status check never does).
    pub(crate) fn supersedes(self) -> bool {
        !matches!(self, Task::Status)
    }

    /// Whether the task may run a login, so it can be cancelled and takes pasted codes.
    pub(crate) fn signs_in(self) -> bool {
        matches!(
            self,
            Task::SignIn(_) | Task::TryAgain | Task::UseFound | Task::UseDifferent
        )
    }

    /// Whether the task starts a login run, so it queues until the install is ready and the
    /// network is up. Every other task (the status check, detection, reuse, sign-out) runs the
    /// local CLI only and is refused with `NotReady` while there is no install.
    pub(crate) fn queues(self) -> bool {
        matches!(self, Task::SignIn(_) | Task::TryAgain | Task::UseDifferent)
    }
}

/// Everything one job needs, moved onto its thread.
pub(crate) struct SigninJob<T> {
    pub(crate) ticket: Ticket,
    pub(crate) task: Task,
    pub(crate) vendor: Vendor,
    pub(crate) config: AgentsConfig,
    /// The agent's install: `node` runs `script`. `None` only for a queued task until its
    /// install is `Ready` ([`run`] fills it in before any run).
    pub(crate) installed: Option<Installed>,
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

/// The job thread's body: for a task that queues, the wait for the install and the network;
/// then the flow, then its last state. `kick` is the slot's kick on each install `Ready`.
pub(crate) fn run<T: Timer + Send + Sync>(
    mut job: SigninJob<T>,
    kick: Option<async_channel::Receiver<()>>,
) {
    if let Some(kick) = kick {
        let queue = Queue {
            ticket: &job.ticket,
            config: &job.config,
            shared: &job.shared,
            timer: &*job.timer,
            kick: &kick,
        };
        match futures::executor::block_on(queue::until_startable(&queue)) {
            Gate::Start(installed) => job.installed = Some(installed),
            Gate::Stop => {
                // Sent only while the job is still the agent's: after `cancel_sign_in`, not once
                // superseded or stopped.
                let cancelled = job.failed(SigninCode::Cancelled);
                job.shared.signin.emit_job(&job.ticket, cancelled, true);
                return;
            }
        }
    }
    let paste = job.paste.take();
    let last = futures::executor::block_on(job.work(paste));
    if job.task.signs_in()
        && matches!(
            last,
            Some(SigninState::SignedIn {
                source: Source::Own,
                ..
            })
        )
        && job.shared.signin.is_job(&job.ticket)
    {
        // "written … when Clauseo's sign-in ends". A failed write leaves no marker, which for
        // Codex reads as reused while the home holds a login file: the safe side.
        drop(marker::write_source(
            &job.config.data_dir,
            &job.ticket.agent,
            &LoginSource::Own,
        ));
    }
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
        // Every run but detection starts in the home, so it must be there; a failure shows as
        // the run not starting.
        drop(crate::write::create_private_dirs_under(
            &self.config.data_dir,
            &self.home,
        ));
        match (self.vendor, self.task) {
            (Vendor::Claude, Task::SignIn(method)) => {
                Some(claude::sign_in(self, method == Method::Console, paste).await)
            }
            (Vendor::Claude, Task::SignOut) => claude::sign_out(self).await,
            // The call refuses it for Claude, which has no callback port.
            (Vendor::Claude, Task::TryAgain) => None,
            (Vendor::Codex, Task::SignIn(method)) => Some(codex::sign_in(self, method).await),
            (Vendor::Codex, Task::TryAgain) => Some(codex::try_again(self).await),
            (Vendor::Codex, Task::SignOut) => codex::sign_out(self).await,
            (_, Task::Status) => found::check(self).await,
            (_, Task::FindLogin) => found::find_login(self).await,
            (_, Task::UseFound) => Some(found::use_found(self).await),
            (_, Task::UseDifferent) => Some(found::use_different(self, paste).await),
            (_, Task::StopUsing(why)) => found::stop_using(self, why),
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

    /// Sends `note` for R20's log; nothing once the job is superseded.
    pub(crate) fn note(&self, note: SigninNote) {
        self.shared.signin.emit_note(&self.ticket, note);
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
    pub(crate) fn spec<A: Into<OsString>>(
        &self,
        args: impl IntoIterator<Item = A>,
        env: BTreeMap<OsString, OsString>,
        limit: Duration,
    ) -> RunSpec {
        // Always set by now: a task without an install is refused or waits in `run`.
        let (node, script) = self
            .installed
            .as_ref()
            .map(|installed| (installed.node.clone(), installed.script.clone()))
            .unwrap_or_default();
        RunSpec {
            node,
            script,
            args: args.into_iter().map(Into::into).collect(),
            env,
            cwd: self.home.clone(),
            limit,
        }
    }
}
