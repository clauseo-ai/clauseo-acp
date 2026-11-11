//! The worker thread: each snapshot agent's first state at start, then the handle's commands in
//! order (installs, pauses, retries, and the sign-in jobs `signin::start_job` starts).

use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentId;

use super::{Command, Inner, Shared, Ticket, first, signin};
use crate::cancel::Cancel;
use crate::config::{AgentsConfig, Network};
use crate::install::{self, Job};
use crate::log::{InstallLog, redact};
use crate::state::{Failure, FailureKind, InstallState, PythonState};
use crate::sysproxy;

/// The worker thread's state.
pub(super) struct Worker<T> {
    pub(super) config: AgentsConfig,
    /// The network every install try reads as it starts; `config.install`'s copy is sign-in's.
    pub(super) network: Arc<RwLock<Network>>,
    /// The installs' children; `signin_launcher` starts sign-in's, clearing the environment.
    pub(super) launcher: Arc<dyn Launcher>,
    pub(super) signin_launcher: Arc<dyn Launcher>,
    pub(super) timer: Arc<T>,
    pub(super) shared: Arc<Shared>,
    /// Each agent's turn lock: a job holds its agent's for its whole run, so a superseded job's
    /// thread has stopped before the next job of that agent works. Another agent's job never
    /// waits on it.
    pub(super) turns: BTreeMap<AgentId, Arc<Mutex<()>>>,
    /// The OS's proxy, once a retry asked for it.
    pub(super) system_proxy: Option<String>,
}

impl<T: Timer + Send + Sync + 'static> Worker<T> {
    pub(super) fn run(mut self, inbox: &mpsc::Receiver<Command>) {
        futures::executor::block_on(first::first_states(&self));
        while let Ok(command) = inbox.recv() {
            match command {
                Command::Install(agent) => self.install(&agent),
                Command::Pause(agent) => self.pause(&agent),
                Command::Retry(agent, use_system_proxy) => self.retry(&agent, use_system_proxy),
                Command::SetNetwork(network) => self.set_network(*network),
                Command::Signin(start) => signin::start_job(&self, *start),
            }
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
                // A Ready agent's job installs Python when it is missing and no job runs (a
                // failed one waits for a retry; a running one fetches Python as its own second
                // step, or is fetching it); otherwise `Ready` again.
                let missing = matches!(inner.python, PythonState::Missing { .. });
                if !inner.jobs.is_empty() || !missing {
                    let installed = inner.installed.get(agent).cloned();
                    Inner::set(&mut inner, &self.shared.events, agent, ready, installed);
                    return;
                }
            }
            if inner.running(agent) {
                return;
            }
        }
        self.start_job(agent);
    }

    fn pause(&self, agent: &AgentId) {
        let mut inner = self.shared.lock();
        inner.stop_job(&self.shared.events, agent, false);
    }

    fn set_network(&mut self, network: Network) {
        self.config.install.set_network(network.clone());
        *self.network.write().unwrap_or_else(PoisonError::into_inner) = network;
    }

    fn retry(&mut self, agent: &AgentId, use_system_proxy: bool) {
        if self.config.snapshot.agent(agent).is_none() {
            return;
        }
        if self.config.managed.contains(agent) {
            self.shared.emit(agent, InstallState::Managed, None);
            return;
        }
        let restartable = {
            let mut inner = self.shared.lock();
            match inner.states.get(agent) {
                Some(
                    InstallState::Failed(_) | InstallState::Waiting { .. } | InstallState::Paused,
                ) => true,
                // Python's retry: a job for a Ready agent installs only Python. While another
                // agent's install runs, nothing starts (as `install`): it fetches Python as its
                // own second step. A job already in its Python step (its agent Ready) is
                // stopped and Python restarted in this agent's job, which pauses no agent.
                Some(InstallState::Ready { .. }) => {
                    let wanted = python_wanted(&inner.python)
                        || matches!(inner.python, PythonState::Waiting { .. });
                    let installing = inner.jobs.keys().any(|running| {
                        !matches!(inner.states.get(running), Some(InstallState::Ready { .. }))
                    });
                    let restart = wanted && !installing;
                    if restart && let Some(owner) = inner.python_owner() {
                        inner.stop_job(&self.shared.events, &owner, true);
                    }
                    restart
                }
                _ => false,
            }
        };
        if !restartable {
            return;
        }
        if use_system_proxy {
            self.system_proxy =
                futures::executor::block_on(sysproxy::system_proxy(&*self.launcher, &*self.timer));
        }
        self.start_job(agent);
    }

    /// Supersedes `agent`'s running job, if any (no other agent's job is touched), then starts
    /// `agent`'s on its own thread; that thread waits for the agent's turn lock before it works.
    fn start_job(&mut self, agent: &AgentId) {
        let ticket = {
            let mut inner = self.shared.lock();
            if inner.closed {
                return;
            }
            inner.stop_job(&self.shared.events, agent, true);
            inner.next_job += 1;
            let ticket = Ticket {
                job: inner.next_job,
                agent: agent.clone(),
                cancel: Arc::new(Cancel::new()),
            };
            inner.add_job(ticket.clone());
            ticket
        };
        let turn = Arc::clone(self.turns.entry(agent.clone()).or_default());
        let job = Job {
            ticket: ticket.clone(),
            config: self.config.clone(),
            network: Arc::clone(&self.network),
            launcher: Arc::clone(&self.launcher),
            timer: Arc::clone(&self.timer),
            shared: Arc::clone(&self.shared),
            turn,
            system_proxy: self.system_proxy.clone(),
            locks: install::Locks::default(),
        };
        let ended = PanicEnd {
            shared: Arc::clone(&self.shared),
            ticket: ticket.clone(),
        };
        let spawned = self
            .shared
            .spawn_job(format!("clauseo-install-{agent}"), move || {
                let _ended = ended;
                install::run(&job);
            });
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

/// Frees a job's slot if its thread panics (a dev or test build; a release build aborts), so its
/// agent can install again and the other agent's Python claim or disk reserve never waits on it:
/// the agent's install `Failed` (or, past its `Ready`, Python back to `Missing`).
struct PanicEnd {
    shared: Arc<Shared>,
    ticket: Ticket,
}

impl Drop for PanicEnd {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.shared.end_after_panic(&self.ticket);
        }
    }
}

/// Whether Python's state asks for an install: `Missing` or `Failed`.
fn python_wanted(python: &PythonState) -> bool {
    matches!(python, PythonState::Missing { .. } | PythonState::Failed(_))
}
