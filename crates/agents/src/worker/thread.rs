//! The worker thread: each snapshot agent's first state at start, then the handle's commands in
//! order (installs, pauses, retries, and the sign-in jobs `signin::start_job` starts).

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
    pub(super) job_lock: Arc<Mutex<()>>,
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
                // failed one waits for a retry); otherwise `Ready` again.
                let missing = matches!(inner.python, PythonState::Missing { .. });
                if inner.current.is_some() || !missing {
                    let installed = inner.installed.get(agent).cloned();
                    Inner::set(&mut inner, &self.shared.events, agent, ready, installed);
                    return;
                }
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
            let inner = self.shared.lock();
            match inner.states.get(agent) {
                Some(
                    InstallState::Failed(_) | InstallState::Waiting { .. } | InstallState::Paused,
                ) => true,
                // Python's retry: a job for a Ready agent installs only Python.
                Some(InstallState::Ready { .. }) => {
                    python_wanted(&inner.python)
                        || matches!(inner.python, PythonState::Waiting { .. })
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

    /// Stops the running job (pausing another agent's), then starts `agent`'s on its own
    /// thread; that thread waits for the job lock before it works.
    fn start_job(&self, agent: &AgentId) {
        let ticket = {
            let mut inner = self.shared.lock();
            if inner.closed {
                return;
            }
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
            network: Arc::clone(&self.network),
            launcher: Arc::clone(&self.launcher),
            timer: Arc::clone(&self.timer),
            shared: Arc::clone(&self.shared),
            turn: Arc::clone(&self.job_lock),
            system_proxy: self.system_proxy.clone(),
            data_lock: Mutex::new(None),
        };
        let spawned = self
            .shared
            .spawn_job(format!("clauseo-install-{agent}"), move || {
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

/// Whether Python's state asks for an install: `Missing` or `Failed`.
fn python_wanted(python: &PythonState) -> bool {
    matches!(python, PythonState::Missing { .. } | PythonState::Failed(_))
}
