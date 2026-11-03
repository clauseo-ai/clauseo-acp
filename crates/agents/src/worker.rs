//! `Agents`: the handle the app holds, and the worker thread behind it.
//!
//! Every call sends a command to the worker and returns at once; the worker gives each agent
//! its first state at start, then runs one job thread per install ([`crate::install`]). Jobs
//! run one after another: each takes the job lock before its work, so a new job starts only
//! once a paused one's thread has stopped. Every event a job sends goes through
//! [`Shared::emit_job`], under the same lock `pause` takes, and is dropped once the job is
//! paused or superseded: a paused job never speaks after its `Paused`.

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentId;

use crate::cancel::Cancel;
use crate::config::AgentsConfig;
use crate::install::{self, Job};
use crate::log::{InstallLog, redact};
use crate::state::{AgentsEvent, Failure, FailureKind, InstallState, Installed};
use crate::{node, receipt, sysproxy};

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
}

/// What the handle, the worker and the jobs share.
#[derive(Debug)]
pub(crate) struct Shared {
    inner: Mutex<Inner>,
    events: async_channel::Sender<AgentsEvent>,
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
    /// The agent it installs.
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
        self.shared.lock().installed.get(agent).cloned()
    }

    fn send(&self, command: Command) {
        // A send fails only once the worker is gone, and then nothing can be done.
        drop(self.commands.send(command));
    }
}

impl Drop for Agents {
    /// Stops the running job, if any; the worker ends once its commands' sender is gone.
    fn drop(&mut self) {
        if let Some(running) = self.shared.lock().current.take() {
            running.cancel.cancel();
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records and sends `state` for `agent`: the worker's own events.
    fn emit(&self, agent: &AgentId, state: InstallState, installed: Option<Installed>) {
        let mut inner = self.lock();
        Inner::set(&mut inner, &self.events, agent, state, installed);
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
        Inner::set(&mut inner, &self.events, &ticket.agent, state, installed);
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
            }
        }
    }

    /// Each snapshot agent's state at start.
    async fn first_states(&self) {
        let config = &self.config;
        let user = node::find_user_node(&config.node_search, &*self.launcher, &*self.timer).await;
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
            if lock.is_some()
                && let Some(hash) = config.snapshot.install_hash(&agent, config.platform)
            {
                let scan = receipt::scan(&config.data_dir, &agent, &hash, config.platform);
                // A folder that will not go now goes at the next start, or under the install's
                // own rename.
                drop(scan.and_then(|scan| receipt::remove_stale(&scan)));
            }
            if paths_ok
                && let Some(installed) = install::ready_install(
                    config,
                    &*self.launcher,
                    &*self.timer,
                    &agent,
                    user.as_ref(),
                )
                .await
            {
                let version = installed.version.clone();
                self.shared
                    .emit(&agent, InstallState::Ready { version }, Some(installed));
                continue;
            }
            let node_needed = user.is_none() && !install::runtime_ready(config);
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
