//! The worker thread: each snapshot agent's first state at start, then the handle's commands in
//! order (installs, pauses, retries, and the sign-in jobs `signin::start_job` starts).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentId;

use super::{Command, Inner, Shared, Ticket, signin};
use crate::cancel::Cancel;
use crate::config::AgentsConfig;
use crate::install::{self, Job};
use crate::log::{InstallLog, redact};
use crate::state::{Failure, FailureKind, InstallState};
use crate::{node, receipt, sysproxy};

/// The worker thread's state.
pub(super) struct Worker<T> {
    pub(super) config: AgentsConfig,
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
            Some(InstallState::Failed(_) | InstallState::Waiting { .. } | InstallState::Paused)
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
