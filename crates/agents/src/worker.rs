//! `Agents`: the handle the app holds, and the worker thread behind it.
//!
//! Every call sends a command to the worker and returns at once; the worker gives each agent
//! its first state at start, then runs one job thread per install ([`crate::install`]). Jobs
//! run one after another: each takes the job lock before its work, so a new job starts only
//! once a paused one's thread has stopped. Every event a job sends goes through
//! [`Shared::emit_job`], under the same lock `pause` takes, and is dropped once the job is
//! paused or superseded: a paused job never speaks after its `Paused`.
//!
//! Sign-in jobs (`signin`) run beside the installs, one per agent, on the model in
//! [`crate::signin::job`]: the calls reserve the agent's job slot, the worker starts the job.
//! Every install `Ready` kicks the agent's queued sign-in, if any (`SigninHub::install_ready`).

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentId;

use crate::cancel::Cancel;
use crate::config::AgentsConfig;
use crate::install::{self, Job};
use crate::log::{InstallLog, redact};
use crate::signin::{Found, Method, PasteCode, SigninError, SigninState};
use crate::state::{AgentsEvent, Failure, FailureKind, InstallState, Installed};
use crate::{node, receipt, sysproxy};

mod signin;

pub(crate) use self::signin::SigninHub;
use self::signin::SigninStart;

/// The agents' installs. Cheap calls: each hands its work to the worker thread.
#[derive(Debug)]
pub struct Agents {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
}

/// What a call asks of the worker.
#[derive(Debug)]
enum Command {
    Install(AgentId),
    Pause(AgentId),
    Retry(AgentId, bool),
    /// Start a reserved sign-in, sign-out or status job.
    Signin(Box<SigninStart>),
}

/// What the handle, the worker and the jobs share.
#[derive(Debug)]
pub(crate) struct Shared {
    inner: Mutex<Inner>,
    events: async_channel::Sender<AgentsEvent>,
    /// Every agent's sign-in state and running sign-in job.
    pub(crate) signin: SigninHub,
}

#[derive(Debug, Default)]
struct Inner {
    states: BTreeMap<AgentId, InstallState>,
    installed: BTreeMap<AgentId, Installed>,
    /// The job that may emit now, if any.
    current: Option<Ticket>,
    next_job: u64,
}

/// One job's identity and pause signal.
#[derive(Debug, Clone)]
pub(crate) struct Ticket {
    /// Unique per `Agents`.
    pub(crate) job: u64,
    /// The agent it installs or signs in.
    pub(crate) agent: AgentId,
    /// Set by a pause or a newer job; the job's emits stop then.
    pub(crate) cancel: Arc<Cancel>,
}

impl Agents {
    /// Starts the worker thread and gives the handle and the events' receiver. The worker
    /// first sends each snapshot agent's state: `Managed` for an id in `config.managed`;
    /// otherwise, once stale install folders are removed, `Ready` for a valid receipt whose
    /// Node still qualifies, else `Missing`.
    pub fn start<T: Timer + Send + Sync + 'static>(
        config: AgentsConfig,
        launcher: Arc<dyn Launcher>,
        timer: T,
    ) -> (Agents, async_channel::Receiver<AgentsEvent>) {
        let (events, receiver) = async_channel::unbounded();
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner::default()),
            signin: SigninHub::new(&config, events.clone()),
            events,
        });
        let (commands, inbox) = mpsc::channel();
        let worker = Worker {
            config,
            launcher,
            timer: Arc::new(timer),
            shared: Arc::clone(&shared),
            job_lock: Arc::new(Mutex::new(())),
            system_proxy: None,
        };
        let spawned = std::thread::Builder::new()
            .name("clauseo-agents".to_owned())
            .spawn(move || worker.run(&inbox));
        // With no worker the events' sender is gone with it, so the receiver reports closed.
        drop(spawned);
        (Agents { commands, shared }, receiver)
    }

    /// Installs `agent`: `Managed` again for IT's runtime, `Ready` again when installed,
    /// nothing when it is installing already. Otherwise any other agent's running install is
    /// paused (its partial files kept) and this one starts, or resumes from its files.
    pub fn install(&self, agent: &AgentId) {
        self.send(Command::Install(agent.clone()));
    }

    /// Pauses `agent`'s install if it is running: `Paused`, its partial files kept.
    pub fn pause(&self, agent: &AgentId) {
        self.send(Command::Pause(agent.clone()));
    }

    /// Starts a `Failed`, `Waiting` or `Paused` install again with a fresh count of tries. With
    /// `use_system_proxy`, the OS's static proxy setting is read now and kept for later
    /// installs (a proxy set in the install settings still wins).
    pub fn retry(&self, agent: &AgentId, use_system_proxy: bool) {
        self.send(Command::Retry(agent.clone(), use_system_proxy));
    }

    /// `agent`'s last state, or `None` for an unknown agent or before the worker set it.
    pub fn state(&self, agent: &AgentId) -> Option<InstallState> {
        self.shared.lock().states.get(agent).cloned()
    }

    /// `agent`'s finished install, when it is `Ready`.
    pub fn installed(&self, agent: &AgentId) -> Option<Installed> {
        self.shared.installed(agent)
    }

    /// Tests only: ends `agent`'s install as an install job would, through the same emit,
    /// `Ready` with `installed` or, for `None`, `Failed` (kind `Failed`, an empty log), so a
    /// test can see what a queued sign-in does when an install finishes.
    #[cfg(feature = "test-support")]
    pub fn finish_install_for_tests(&self, agent: &AgentId, installed: Option<Installed>) {
        let state = match &installed {
            Some(installed) => InstallState::Ready {
                version: installed.version.clone(),
            },
            None => InstallState::Failed(Failure {
                kind: FailureKind::Failed,
                need_mb: None,
                free_mb: None,
                log: InstallLog::new(agent),
            }),
        };
        self.shared.emit(agent, state, installed);
    }

    /// Signs `agent` in with `method` (Claude `ClaudeAi` or `Console`, Codex `Browser` or
    /// `DeviceCode`): a hidden login run in Clauseo's own folder, reported as `Waiting` (or
    /// Codex's `DeviceCode`) and then `SignedIn` or `Failed`, with `OpenUrl` for the page
    /// Claude would open (Codex opens its own). Codex's `Browser` first checks that port 1455 is
    /// free, else `Failed { code: PortBusy }` and no run. A sign-in or sign-out already going
    /// for `agent` is stopped first, silently: "Didn't open?" is a new sign-in. While the
    /// install is not `Ready`, or the agent's sign-in host does not resolve (offline, looked up
    /// again every 30 s), the state is `Queued` and the run starts by itself once both hold;
    /// `cancel_sign_in` ends the wait. Refused when the agent is unknown, the method is another
    /// agent's, or the caller gave it no home.
    pub fn sign_in(&self, agent: &AgentId, method: Method) -> Result<(), SigninError> {
        let start = self
            .shared
            .signin
            .sign_in(agent, method, self.installed(agent));
        self.start_signin(start)
    }

    /// Codex's "Try again" after `signin.port_busy`: probes port 1455 every 2 s and starts the
    /// browser sign-in once it is free, as `sign_in(agent, Browser)` then does. The state is
    /// unchanged while it waits; `cancel_sign_in` stops the wait. Queued as `sign_in` is.
    /// Refused for Claude (it has no callback port) and as `sign_in` is.
    pub fn try_again(&self, agent: &AgentId) -> Result<(), SigninError> {
        let start = self.shared.signin.try_again(agent, self.installed(agent));
        self.start_signin(start)
    }

    /// Cancels `agent`'s sign-in run, or its queued sign-in: a child's tree is killed and the
    /// state becomes `Failed { code: Cancelled }`. Refused when no sign-in is going or queued.
    pub fn cancel_sign_in(&self, agent: &AgentId) -> Result<(), SigninError> {
        self.shared.signin.cancel(agent)
    }

    /// Writes `code` and a newline to `agent`'s login run's stdin (Claude's paste route), then
    /// drops it, zeroed. It is never stored, logged or shown. Refused when no sign-in run is
    /// going (a queued sign-in has no run yet), and with `WrongMethod` for Codex, whose logins
    /// read no code.
    pub fn paste_code(&self, agent: &AgentId, code: PasteCode) -> Result<(), SigninError> {
        self.shared.signin.paste(agent, code)
    }

    /// Signs `agent` out of Clauseo's own login (the vendor's logout), then checks status.
    /// Refused for a reused login, the lawyer's own: `SignedIn { source: Reused }` is left with
    /// `stop_using_login`. The job checks `login-source` again, and for Codex refuses while the
    /// home's login file is the lawyer's (same file id). A sign-in going is stopped first,
    /// silently.
    pub fn sign_out(&self, agent: &AgentId) -> Result<(), SigninError> {
        let start = self.shared.signin.sign_out(agent, self.installed(agent));
        self.start_signin(start)
    }

    /// The status check for `agent`, at start and when R20 asks ("a window shows Plans"): a
    /// recorded reuse whose login still holds is `SignedIn { source: Reused }` with no question
    /// (or `SignedOut` with why reuse stopped); else Clauseo's own login, `SignedIn { source: Own
    /// }`; else, unless `no-reuse` is there, detection: `LoginFound` or `SignedOut` with what
    /// was seen. Unchanged when status cannot tell. Nothing new runs while a sign-in or sign-out
    /// is going, since that reports by itself.
    pub fn check_status(&self, agent: &AgentId) -> Result<(), SigninError> {
        let start = self
            .shared
            .signin
            .check_status(agent, self.installed(agent));
        self.start_signin(start)
    }

    /// UD2's "Look for my Claude Code login" or "Look for my Codex login": deletes `no-reuse` and
    /// runs the status check again, which detects the lawyer's login (`LoginFound`, or
    /// `SignedOut` with what was seen) unless Clauseo's own login is signed in. Refused as
    /// `check_status` is.
    pub fn find_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        let start = self.shared.signin.find_login(agent, self.installed(agent));
        self.start_signin(start)
    }

    /// The lawyer's "Use it" on a `LoginFound`: detection runs again (another account than the
    /// one shown comes back as `LoginFound`, unused); then Claude's status under the reused
    /// session's environment must say signed in (else the run-time fallback: the
    /// `claude_reuse_fallback` note and `SignedOut`), or Codex's login file is hard-linked into
    /// Clauseo's home (a failed link: the `codex_link_failed` note and Clauseo's own browser
    /// sign-in); then `login-source` records `reused` with the accepted email, and the state is
    /// `SignedIn { source: Reused }`. Refused with `NothingFound` unless the state is
    /// `LoginFound`.
    pub fn use_found_login(&self, agent: &AgentId) -> Result<(), SigninError> {
        let start = self.shared.signin.use_found(agent, self.installed(agent));
        self.start_signin(start)
    }

    /// "Sign in with a different account": writes `no-reuse` and starts Clauseo's own sign-in
    /// (Claude's `--claudeai`, Codex's browser), reported as `sign_in` reports; the lawyer's CLI
    /// login is untouched. Queued as `sign_in` is. Refused for a reused login
    /// (`stop_using_login` first) and as `sign_in` is.
    pub fn use_different_account(&self, agent: &AgentId) -> Result<(), SigninError> {
        let start = self
            .shared
            .signin
            .use_different(agent, self.installed(agent));
        self.start_signin(start)
    }

    /// Stops reusing the lawyer's login, and never signs it out: no `auth logout` or `codex
    /// logout` runs. With no reason (UD2's "Stop using it here") `no-reuse` is written, so the
    /// next start does not reuse again without asking; with a reason (`ReusedFailed`,
    /// `CodexLinkSplit`, …) only the reuse record is cleared, so the next start shows
    /// `LoginFound` again. The state becomes `SignedOut { found: why }`. For Codex the home's
    /// link is removed too ([`crate::unlink_reused_login`]): R20 calls this in its leave order
    /// once every Codex process using the home has stopped, then `sign_in` and relaunch.
    /// Refused with `NotReused` for Clauseo's own login, and as `check_status` is.
    pub fn stop_using_login(&self, agent: &AgentId, why: Option<Found>) -> Result<(), SigninError> {
        let start = self
            .shared
            .signin
            .stop_using(agent, why, self.installed(agent));
        self.start_signin(start)
    }

    /// `agent`'s last sign-in state, or `None` before any.
    pub fn signin_state(&self, agent: &AgentId) -> Option<SigninState> {
        self.shared.signin.state(agent)
    }

    /// Hands a reserved sign-in job to the worker.
    fn start_signin(
        &self,
        start: Result<Option<SigninStart>, SigninError>,
    ) -> Result<(), SigninError> {
        if let Some(start) = start? {
            self.send(Command::Signin(Box::new(start)));
        }
        Ok(())
    }

    fn send(&self, command: Command) {
        // A send fails only once the worker is gone, and then nothing can be done.
        drop(self.commands.send(command));
    }
}

impl Drop for Agents {
    /// Stops the running jobs, install and sign-ins; the worker ends once its commands' sender
    /// is gone.
    fn drop(&mut self) {
        if let Some(running) = self.shared.lock().current.take() {
            running.cancel.cancel();
        }
        self.shared.signin.stop_all();
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `agent`'s finished install, when it is `Ready`.
    pub(crate) fn installed(&self, agent: &AgentId) -> Option<Installed> {
        self.lock().installed.get(agent).cloned()
    }

    /// Records and sends `state` for `agent`: the worker's own events.
    fn emit(&self, agent: &AgentId, state: InstallState, installed: Option<Installed>) {
        let ready = matches!(state, InstallState::Ready { .. });
        Inner::set(&mut self.lock(), &self.events, agent, state, installed);
        if ready {
            self.signin.install_ready(agent);
        }
    }

    /// Records and sends `state` for the job of `ticket`, unless it was paused or superseded:
    /// then nothing happens and `false` comes back. `ends` marks the job's last event (`Ready`
    /// or `Failed`), after which it is no longer the running job.
    pub(crate) fn emit_job(
        &self,
        ticket: &Ticket,
        state: InstallState,
        installed: Option<Installed>,
        ends: bool,
    ) -> bool {
        let mut inner = self.lock();
        if ticket.cancel.is_cancelled() {
            return false;
        }
        if ends
            && inner
                .current
                .as_ref()
                .is_some_and(|current| current.job == ticket.job)
        {
            inner.current = None;
        }
        let ready = matches!(state, InstallState::Ready { .. });
        Inner::set(&mut inner, &self.events, &ticket.agent, state, installed);
        drop(inner);
        // After the lock: a queued sign-in reads the install again once kicked.
        if ready {
            self.signin.install_ready(&ticket.agent);
        }
        true
    }
}

impl Inner {
    fn set(
        inner: &mut Inner,
        events: &async_channel::Sender<AgentsEvent>,
        agent: &AgentId,
        state: InstallState,
        installed: Option<Installed>,
    ) {
        match installed {
            Some(installed) => {
                inner.installed.insert(agent.clone(), installed);
            }
            None if !matches!(state, InstallState::Ready { .. }) => {
                inner.installed.remove(agent);
            }
            None => {}
        }
        inner.states.insert(agent.clone(), state.clone());
        // Fails only when every receiver is gone; nobody is listening then.
        drop(events.try_send(AgentsEvent::Install {
            agent: agent.clone(),
            state,
        }));
    }

    /// Stops the running job. It becomes `Paused`, unless it is `next`'s own job (a retry
    /// while it waits), which is superseded quietly.
    fn stop_current(
        &mut self,
        events: &async_channel::Sender<AgentsEvent>,
        next: Option<&AgentId>,
    ) {
        let Some(running) = self.current.take() else {
            return;
        };
        running.cancel.cancel();
        if next != Some(&running.agent) {
            Inner::set(self, events, &running.agent, InstallState::Paused, None);
        }
    }
}

/// The worker thread's state.
struct Worker<T> {
    config: AgentsConfig,
    launcher: Arc<dyn Launcher>,
    timer: Arc<T>,
    shared: Arc<Shared>,
    job_lock: Arc<Mutex<()>>,
    /// The OS's proxy, once a retry asked for it.
    system_proxy: Option<String>,
}

impl<T: Timer + Send + Sync + 'static> Worker<T> {
    fn run(mut self, inbox: &mpsc::Receiver<Command>) {
        futures::executor::block_on(self.first_states());
        while let Ok(command) = inbox.recv() {
            match command {
                Command::Install(agent) => self.install(&agent),
                Command::Pause(agent) => self.pause(&agent),
                Command::Retry(agent, use_system_proxy) => self.retry(&agent, use_system_proxy),
                Command::Signin(start) => signin::start_job(&self, *start),
            }
        }
    }

    /// Each snapshot agent's state at start.
    async fn first_states(&self) {
        let config = &self.config;
        let search =
            node::search_user_node(&config.node_search, &*self.launcher, &*self.timer).await;
        let agents: Vec<AgentId> = config.snapshot.agents().map(|e| e.id.clone()).collect();
        // The clean-up below runs only with `agents/.lock` held: another Clauseo installing now
        // may own the temp folders it would remove. A relative data folder is never touched.
        let paths_ok = install::check_paths(config).is_ok();
        let lock = paths_ok
            .then(|| {
                crate::write::try_lock_agents(&config.data_dir)
                    .ok()
                    .flatten()
            })
            .flatten();
        for agent in agents {
            if config.managed.contains(&agent) {
                self.shared.emit(&agent, InstallState::Managed, None);
                continue;
            }
            #[cfg(feature = "test-support")]
            if let Some(installed) = config.ready_for_tests.get(&agent) {
                let version = installed.version.clone();
                self.shared.emit(
                    &agent,
                    InstallState::Ready { version },
                    Some(installed.clone()),
                );
                continue;
            }
            if lock.is_some()
                && let Some(hash) = config.snapshot.install_hash(&agent, config.platform)
            {
                let scan = receipt::scan(&config.data_dir, &agent, &hash, config.platform);
                // A folder that will not go now goes at the next start, or under the install's
                // own rename.
                drop(scan.and_then(|scan| receipt::remove_stale(&scan)));
            }
            if paths_ok
                && let Some(installed) =
                    install::ready_install(config, &*self.launcher, &*self.timer, &agent, &search)
                        .await
            {
                let version = installed.version.clone();
                self.shared
                    .emit(&agent, InstallState::Ready { version }, Some(installed));
                continue;
            }
            let node_needed = search.found.is_none() && !install::runtime_ready(config);
            let download_bytes = config
                .snapshot
                .download_bytes(&agent, config.platform, node_needed)
                .unwrap_or(0);
            self.shared
                .emit(&agent, InstallState::Missing { download_bytes }, None);
        }
    }

    fn install(&mut self, agent: &AgentId) {
        if self.config.snapshot.agent(agent).is_none() {
            return;
        }
        if self.config.managed.contains(agent) {
            self.shared.emit(agent, InstallState::Managed, None);
            return;
        }
        {
            let mut inner = self.shared.lock();
            if let Some(ready @ InstallState::Ready { .. }) = inner.states.get(agent).cloned() {
                let installed = inner.installed.get(agent).cloned();
                Inner::set(&mut inner, &self.shared.events, agent, ready, installed);
                return;
            }
            if inner
                .current
                .as_ref()
                .is_some_and(|current| current.agent == *agent)
            {
                return;
            }
        }
        self.start_job(agent);
    }

    fn pause(&self, agent: &AgentId) {
        let mut inner = self.shared.lock();
        if inner
            .current
            .as_ref()
            .is_some_and(|current| current.agent == *agent)
        {
            inner.stop_current(&self.shared.events, None);
        }
    }

    fn retry(&mut self, agent: &AgentId, use_system_proxy: bool) {
        if self.config.snapshot.agent(agent).is_none() {
            return;
        }
        if self.config.managed.contains(agent) {
            self.shared.emit(agent, InstallState::Managed, None);
            return;
        }
        let restartable = matches!(
            self.shared.lock().states.get(agent),
            Some(
                InstallState::Failed(_)
                    | InstallState::Waiting { .. }
                    | InstallState::Paused
                    | InstallState::Missing { .. }
            )
        );
        if !restartable {
            return;
        }
        if use_system_proxy {
            self.system_proxy =
                futures::executor::block_on(sysproxy::system_proxy(&*self.launcher, &*self.timer));
        }
        self.start_job(agent);
    }

    /// Stops the running job (pausing another agent's), then starts `agent`'s on its own
    /// thread; that thread waits for the job lock before it works.
    fn start_job(&self, agent: &AgentId) {
        let ticket = {
            let mut inner = self.shared.lock();
            inner.stop_current(&self.shared.events, Some(agent));
            inner.next_job += 1;
            let ticket = Ticket {
                job: inner.next_job,
                agent: agent.clone(),
                cancel: Arc::new(Cancel::new()),
            };
            inner.current = Some(ticket.clone());
            ticket
        };
        let job = Job {
            ticket: ticket.clone(),
            config: self.config.clone(),
            launcher: Arc::clone(&self.launcher),
            timer: Arc::clone(&self.timer),
            shared: Arc::clone(&self.shared),
            turn: Arc::clone(&self.job_lock),
            system_proxy: self.system_proxy.clone(),
            data_lock: Mutex::new(None),
        };
        let spawned = std::thread::Builder::new()
            .name(format!("clauseo-install-{agent}"))
            .spawn(move || install::run(&job));
        if let Err(err) = spawned {
            let mut log = InstallLog::new(agent);
            let detail = format!("the install could not start: {err}");
            log.push(redact(&detail, &self.config.data_dir));
            log.push(format!(
                "result: {} after 0 tries",
                FailureKind::Failed.word()
            ));
            let failure = Failure {
                kind: FailureKind::Failed,
                need_mb: None,
                free_mb: None,
                log,
            };
            self.shared
                .emit_job(&ticket, InstallState::Failed(failure), None, true);
        }
    }
}
