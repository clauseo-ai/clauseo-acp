//! `Agents`: the handle the app holds, and the worker thread behind it.
//!
//! Every call sends a command to the worker and returns at once; the worker gives each agent
//! its first state at start, then runs one job thread per install ([`crate::install`]). Each
//! agent has its own job slot, so two agents install at once (decision 0033, "Two installs at
//! once", 2026-11-11): one agent's install, pause or retry never stops another's. A new job for
//! an agent supersedes that agent's running one, and takes the agent's turn lock before its
//! work, so it starts only once the superseded job's thread has stopped. What the two share on
//! disk (the Node runtime, Python, the folder's lock files) is held through `agents/.lock-*`
//! (`install.rs`), and the free space is counted for both (`worker/space.rs`). Every event
//! a job sends goes through [`Shared::emit_job`], under the same lock `pause` takes, and is
//! dropped once the job is paused or superseded: a paused job never speaks after its `Paused`.
//!
//! Sign-in jobs (`signin`) run beside the installs, one per agent, on the model in
//! [`crate::signin::job`]: the calls reserve the agent's job slot, the worker starts the job.
//! Every install `Ready` kicks the agent's queued sign-in, if any (`SigninHub::install_ready`).
//!
//! Quitting (#386): the job threads are counted as they start and end, and
//! [`Agents::shutdown`] cancels every job and waits, within a bound, for the count to reach 0.
//! An install's thread drops its child's handle as it sees the cancel, which kills the child's
//! tree (npm leads its own process group). A sign-in job is stopped gracefully instead, since a
//! Codex run killed while it saves its login file leaves the file cut (`signin::stopping`): its
//! runs end as their kinds allow, and what still runs [`KILL_MARGIN`] before the bound is killed,
//! so an ended thread has left no child. Once shut down or dropped, no job starts; the drop kills
//! at once.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentId;

use crate::cancel::Cancel;
use crate::config::{AgentsConfig, Network};
use crate::signin::{Found, Method, PasteCode, SigninError, SigninState};
use crate::state::{AgentsEvent, InstallState, Installed, PythonState};

mod first;
mod python_log;
mod shared;
mod signin;
pub(crate) mod space;
mod stop;
mod switch;
mod thread;

pub(crate) use self::signin::SigninHub;
use self::signin::SigninStart;
pub use self::switch::OwnLogin;
use self::switch::Switch;
use self::thread::Worker;

/// How long before `shutdown`'s bound the sign-in runs still going are killed: the time their
/// threads then take to end, inside the bound.
pub(crate) const KILL_MARGIN: Duration = Duration::from_millis(500);

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
    /// Replace the proxies and CA files the next jobs take.
    SetNetwork(Box<Network>),
    /// Start a reserved sign-in, sign-out or status job.
    Signin(Box<SigninStart>),
}

/// What the handle, the worker and the jobs share.
#[derive(Debug)]
pub(crate) struct Shared {
    inner: Mutex<Inner>,
    /// How many job threads run, installs and sign-ins; `job_ended` wakes `shutdown` as one
    /// ends.
    jobs: Mutex<usize>,
    job_ended: Condvar,
    /// Held through each disk check (`space.rs`), so two tries never both count on one byte.
    space: Mutex<()>,
    events: async_channel::Sender<AgentsEvent>,
    /// Every agent's sign-in state and running sign-in job.
    pub(crate) signin: SigninHub,
}

#[derive(Debug)]
struct Inner {
    states: BTreeMap<AgentId, InstallState>,
    /// The Python runtime's state, one for every agent.
    python: PythonState,
    /// The size of Python's archive: a paused Python download's `Missing`.
    python_bytes: u64,
    /// When Python's install job began to work, for its log's `took_ms`.
    python_began: Option<std::time::Duration>,
    /// The clock `python_began` reads.
    ticks: Ticks,
    installed: BTreeMap<AgentId, Installed>,
    /// The install jobs that may emit now: one per agent at most, so two agents install at
    /// once and one's job never stops another's.
    jobs: BTreeMap<AgentId, Slot>,
    /// The job installing Python now, if any: Python is one for the computer, so one job at a
    /// time installs it, and another reaching its agent's `Ready` meanwhile ends there.
    python_job: Option<u64>,
    next_job: u64,
    /// Each agent's tries in its last install job that ended, the last one included.
    tries: BTreeMap<AgentId, u32>,
    /// Set by `shutdown` or the drop: no job starts after it.
    closed: bool,
}

/// The config's monotonic clock, for the lifecycle lines' `took_ms`.
#[derive(Clone)]
pub(crate) struct Ticks(Arc<dyn crate::config::Clock>);

impl Ticks {
    pub(crate) fn new(clock: &Arc<dyn crate::config::Clock>) -> Ticks {
        Ticks(Arc::clone(clock))
    }

    /// The monotonic reading now.
    pub(crate) fn now(&self) -> std::time::Duration {
        self.0.monotonic()
    }

    /// Milliseconds from `began`, a reading of [`Ticks::now`], to now.
    pub(crate) fn since_ms(&self, began: std::time::Duration) -> u64 {
        u64::try_from(self.now().saturating_sub(began).as_millis()).unwrap_or(u64::MAX)
    }
}

impl Default for Ticks {
    fn default() -> Ticks {
        Ticks(Arc::new(crate::config::SystemClock))
    }
}

impl std::fmt::Debug for Ticks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ticks")
    }
}

/// One running install job: its ticket and the free space its try counts on.
#[derive(Debug)]
struct Slot {
    ticket: Ticket,
    /// What its try's disk check counted on, once passed: other tries' checks count what of
    /// it is not written yet.
    reserve: space::Reserve,
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
    /// Node still qualifies, else `Missing`. `launcher` starts every child, so it must clear
    /// the environment, as sign-in needs.
    pub fn start<T: Timer + Send + Sync + 'static>(
        config: AgentsConfig,
        launcher: Arc<dyn Launcher>,
        timer: T,
    ) -> (Agents, async_channel::Receiver<AgentsEvent>) {
        Self::start_with_launchers(config, Arc::clone(&launcher), launcher, timer)
    }

    /// [`Agents::start`] with `install` for installs' children (it may inherit the environment:
    /// Windows needs `SystemRoot` and `PATH`) and `signin`, made with `clear_env`, for sign-in's.
    pub fn start_with_launchers<T: Timer + Send + Sync + 'static>(
        config: AgentsConfig,
        install: Arc<dyn Launcher>,
        signin: Arc<dyn Launcher>,
        timer: T,
    ) -> (Agents, async_channel::Receiver<AgentsEvent>) {
        let (events, receiver) = async_channel::unbounded();
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                ticks: Ticks::new(&config.clock),
                ..Inner::default()
            }),
            jobs: Mutex::new(0),
            job_ended: Condvar::new(),
            space: Mutex::new(()),
            signin: SigninHub::new(&config, events.clone()),
            events,
        });
        let (commands, inbox) = mpsc::channel();
        let network = Arc::new(RwLock::new(config.install.network()));
        let worker = Worker {
            config,
            network,
            launcher: install,
            signin_launcher: signin,
            timer: Arc::new(timer),
            shared: Arc::clone(&shared),
            turns: BTreeMap::new(),
            system_proxy: None,
        };
        let spawned = std::thread::Builder::new()
            .name("clauseo-agents".to_owned())
            .spawn(move || worker.run(&inbox));
        // With no worker the events' sender is gone with it, so the receiver reports closed.
        drop(spawned);
        (Agents { commands, shared }, receiver)
    }

    /// Installs `agent`: `Managed` again for IT's runtime, `Ready` again when installed (and
    /// Python's install started when it is `Missing` and no job runs), nothing when
    /// it is installing already. Otherwise this one starts, or resumes from its files, beside
    /// any other agent's running install, which goes on untouched.
    pub fn install(&self, agent: &AgentId) {
        self.send(Command::Install(agent.clone()));
    }

    /// Pauses `agent`'s install if it is running: `Paused`, its partial files kept.
    pub fn pause(&self, agent: &AgentId) {
        self.send(Command::Pause(agent.clone()));
    }

    /// Starts a `Failed`, `Waiting` or `Paused` install again with a fresh count of tries, or,
    /// for a `Ready` agent, Python's install when it is `Missing`, `Waiting` or `Failed`:
    /// Python is one for the computer, so a retry of any `Ready` agent retries it, and that
    /// agent's own install is not redone (its job finds its receipt and goes to Python). While
    /// another agent's install runs, such a retry starts nothing and pauses nothing: that
    /// install fetches Python as its own second step. With
    /// `use_system_proxy`, the OS's static proxy setting is read now and kept for later
    /// installs (a proxy set in the install settings still wins).
    pub fn retry(&self, agent: &AgentId, use_system_proxy: bool) {
        self.send(Command::Retry(agent.clone(), use_system_proxy));
    }

    /// Replaces the proxies and CA files (`network.*`, resolved by the caller): every sign-in
    /// job started after this takes them, and every install try, a running job's next one too
    /// (one waiting offline); a running sign-in keeps its own.
    pub fn set_network(&self, network: Network) {
        self.send(Command::SetNetwork(Box::new(network)));
    }

    /// `agent`'s last state, or `None` for an unknown agent or before the worker set it.
    pub fn state(&self, agent: &AgentId) -> Option<InstallState> {
        self.shared.lock().states.get(agent).cloned()
    }

    /// The Python runtime's last state: `Unavailable` where this build has no Python, and
    /// before the worker set it.
    pub fn python_state(&self) -> PythonState {
        self.shared.python_state()
    }

    /// `agent`'s finished install, when it is `Ready`.
    pub fn installed(&self, agent: &AgentId) -> Option<Installed> {
        self.shared.installed(agent)
    }

    /// How many tries `agent`'s last install job that ended made, the one that ended it
    /// included: set as it sends `Ready` (0 when it found the agent installed already) or
    /// `Failed`. `None` before any job ended.
    pub fn install_tries(&self, agent: &AgentId) -> Option<u32> {
        self.shared.lock().tries.get(agent).copied()
    }

    /// Stops everything for the app's quit, within `wait`: no job starts from now on, and every
    /// install job is cancelled (each kills its child's tree as it sees it). Every sign-in job is
    /// stopped gracefully: each run ends as its kind allows (a status or a logout is awaited,
    /// Codex's browser login gets `GET /cancel`, an app-server its stdin closed), and a job still
    /// running [`KILL_MARGIN`] before `wait` ends is killed, so its thread can end inside `wait`.
    /// Then the threads are waited for to the end of `wait`. Returns whether every one ended in
    /// time; a thread still running is left to end by itself. Blocks, so it runs off any UI
    /// thread.
    pub fn shutdown(&self, wait: Duration) -> bool {
        self.close_installs();
        let stopped = self.shared.signin.stop_all_gracefully();
        if stopped.is_empty() {
            return self.shared.wait_for_jobs(wait);
        }
        // The jobs wind down side by side: one wait for them all, then each still going dies.
        if self.shared.wait_for_jobs(wait.saturating_sub(KILL_MARGIN)) {
            return true;
        }
        for job in &stopped {
            job.wait_or_kill(Duration::ZERO);
        }
        self.shared.wait_for_jobs(KILL_MARGIN)
    }

    /// Closes the worker to new jobs and cancels every install job.
    fn close_installs(&self) {
        let mut inner = self.shared.lock();
        inner.closed = true;
        inner.python_job = None;
        for (_, running) in std::mem::take(&mut inner.jobs) {
            running.ticket.cancel.cancel();
        }
    }

    /// Closes the worker to new jobs and cancels every running one at once, install and
    /// sign-ins.
    fn stop(&self) {
        self.close_installs();
        self.shared.signin.stop_all();
    }

    /// Tests only: ends `agent`'s install as an install job would, through the same emit,
    /// `Ready` with `installed` or, for `None`, `Failed` (kind `Failed`, an empty log), so a
    /// test can see what a queued sign-in does when an install finishes.
    #[cfg(feature = "test-support")]
    pub fn finish_install_for_tests(&self, agent: &AgentId, installed: Option<Installed>) {
        use crate::log::InstallLog;
        use crate::state::{Failure, FailureKind};

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
    ///
    /// A plan sign-in (any method but Claude's `Console`) that finds Clauseo's own login not
    /// signed in detects the lawyer's own login again, by a stat: found, and with no `no-reuse`,
    /// no reuse recorded and (Codex) no login file in Clauseo's home, the job ends `LoginFound`
    /// with no login run, so "Use it" is offered rather than the browser opened.
    ///
    /// For Codex, refused with `OwnLoginPresent` while the login would replace Clauseo's own
    /// ([`Agents::replaces_own_login`]): `codex login` signs that account out first.
    pub fn sign_in(&self, agent: &AgentId, method: Method) -> Result<(), SigninError> {
        let installed = self.installed(agent);
        let start = self
            .shared
            .signin
            .sign_in(agent, method, installed, Switch::Refuse);
        self.start_signin(start)
    }

    /// `sign_in` once the lawyer confirmed the switch to another account: it may replace
    /// Clauseo's own Codex login.
    pub fn sign_in_switching(&self, agent: &AgentId, method: Method) -> Result<(), SigninError> {
        let installed = self.installed(agent);
        let start = self
            .shared
            .signin
            .sign_in(agent, method, installed, Switch::Confirmed);
        self.start_signin(start)
    }

    /// Whether a Codex `login` for `agent` now would replace Clauseo's own login: its
    /// `login-source` says `own` and its home holds the login file (a stat, never an open).
    /// Such a sign-in needs the lawyer's yes, then `sign_in_switching`.
    pub fn replaces_own_login(&self, agent: &AgentId) -> bool {
        self.shared.signin.replaces_own_login(agent)
    }

    /// The Clauseo-own login [`Agents::replaces_own_login`] finds, by a stat of its file (file
    /// id, size, modified time; never opened): `None` when there is none to replace. A caller
    /// that keeps the lawyer's yes past the moment it was given compares it again before using
    /// that yes, so the yes never covers a login it was not given for.
    pub fn own_login(&self, agent: &AgentId) -> Option<OwnLogin> {
        self.shared.signin.own_login(agent)
    }

    /// Codex's "Try again" after `signin.port_busy`: probes port 1455 every 2 s and starts the
    /// browser sign-in once it is free, as `sign_in(agent, Browser)` then does. The state is
    /// unchanged while it waits; `cancel_sign_in` stops the wait. Queued as `sign_in` is.
    /// Refused for Claude (it has no callback port) and as `sign_in` is.
    pub fn try_again(&self, agent: &AgentId) -> Result<(), SigninError> {
        let start = self.shared.signin.try_again(agent, self.installed(agent));
        self.start_signin(start)
    }

    /// Cancels `agent`'s sign-in run, or its queued sign-in, and returns at once. The run ends as
    /// its kind allows (Codex's browser login gets `GET /cancel`; a status is awaited) and is
    /// killed 2 s on if it is still going; Codex's device-code login and Claude's login are killed
    /// at once. Then, for Codex, a status check says how it stands (its `login` deleted the
    /// login it replaced): the state becomes `Failed { code: Cancelled }` while signed out.
    /// Refused when no sign-in is going or queued.
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
        let installed = self.installed(agent);
        let start = self
            .shared
            .signin
            .use_different(agent, installed, Switch::Refuse);
        self.start_signin(start)
    }

    /// `use_different_account` once the lawyer confirmed the switch: it may replace Clauseo's
    /// own Codex login.
    pub fn use_different_account_switching(&self, agent: &AgentId) -> Result<(), SigninError> {
        let installed = self.installed(agent);
        let start = self
            .shared
            .signin
            .use_different(agent, installed, Switch::Confirmed);
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

    /// Whether `agent`'s job is a sign-in, going or queued, from the call that reserves it: the
    /// job [`Agents::cancel_sign_in`] would end, and one `stop_using_login` would replace. R20's
    /// automatic leave asks before it calls `stop_using_login`, since `signin_state` cannot
    /// tell a reserved sign-in from none until its first state.
    pub fn sign_in_going(&self, agent: &AgentId) -> bool {
        self.shared.signin.going(agent)
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
    /// Stops the running jobs, install and sign-ins, as `shutdown` does but without waiting:
    /// each job's thread kills its child's tree as it sees the cancel. The worker ends once its
    /// commands' sender is gone.
    fn drop(&mut self) {
        self.stop();
    }
}
