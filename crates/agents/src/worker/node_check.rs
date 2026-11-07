//! The start's first states, and its Node checks on their own thread, so neither an agent's
//! first `Ready` nor the `Agents` commands wait for a `node --version` (each up to 10 s, and
//! slow under a virus scanner).
//!
//! The worker first sends `Managed`, and `Ready` for every agent whose receipt holds
//! ([`receipt_install`]); launches and sign-ins take that install at once, never waiting for
//! the check (a Node that then cannot start fails that launch or sign-in run, never hangs it).
//! The check then runs the `PATH` search and [`install::ready_install`]
//! as the start always did, per agent:
//!
//! - a receipt agent whose Node still qualifies keeps its `Ready` (sent again only when the
//!   receipt was rewritten to another Node);
//! - one whose Node does not becomes `Failed` (kind `SetupFailed`, code
//!   `agent.install_setup_failed`) with a fixed log, so `installed` is `None` from then on;
//!   `retry` repairs it, and so does a sign-in press, whose sign-in then starts at `Ready`;
//! - every other agent gets its first state, `Ready` or `Missing`, from it.
//!
//! A state the check would replace is left alone once anything else changed it meanwhile (an
//! install started, say). The stop cancels the check, which drops any `node --version` child.

use std::collections::BTreeMap;
use std::pin::pin;
use std::sync::Arc;

use clauseo_process::{Launcher, Timer};
use clauseo_protocol::AgentId;
use futures::future::select;

use super::{Agents, Inner, Shared, SigninStart, Worker};
use crate::cancel::Cancel;
use crate::config::AgentsConfig;
use crate::install;
use crate::log::InstallLog;
use crate::signin::SigninError;
use crate::state::{Failure, FailureKind, InstallState, Installed};
use crate::{node, receipt};

/// The log line of an agent whose receipt's Node failed the start's check.
const FAILED_LINE: &str = "the Node this agent runs on did not pass its check at start";

/// `Inner`'s part for the check: its cancel while it runs, and each agent it turned `Failed`
/// with that state, which a sign-in press repairs ([`repair_for_sign_in`]).
#[derive(Debug, Default)]
pub(super) struct Slot {
    cancel: Option<Arc<Cancel>>,
    failed: BTreeMap<AgentId, InstallState>,
}

impl Slot {
    /// The stop: the check ends where it is.
    pub(super) fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
    }
}

/// A sign-in or "different account" press accepted (`start` reserved a job) for an agent still
/// in the `Failed` the start's check gave it: the press starts the repair, as `retry` does, and
/// the sign-in, queued, starts by itself at `Ready`.
pub(super) fn repair_for_sign_in(
    agents: &Agents,
    agent: &AgentId,
    start: &Result<Option<SigninStart>, SigninError>,
) {
    if !matches!(start, Ok(Some(_))) {
        return;
    }
    let repair = {
        let mut inner = agents.shared.lock();
        let failed = inner.node_check.failed.remove(agent);
        failed.is_some() && inner.states.get(agent) == failed.as_ref()
    };
    if repair {
        tracing::info!(%agent, "agent sign-in: pressed after a failed Node check; the repair starts");
        agents.retry(agent, false);
    }
}

/// Each snapshot agent's state at start: `Managed`, or `Ready` from a receipt alone; every
/// Node check, and the other agents' states, run on the background check's thread.
pub(super) fn first_states<T: Timer + Send + Sync + 'static>(worker: &Worker<T>) {
    let config = &worker.config;
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
    if lock.is_some() {
        // What a try cut short by the last quit left in the npm cache.
        install::empty_npm_cache(&config.data_dir);
    }
    let mut check = Check {
        from_receipt: Vec::new(),
        undecided: Vec::new(),
        paths_ok,
    };
    for agent in agents {
        if config.managed.contains(&agent) {
            worker.shared.emit(&agent, InstallState::Managed, None);
            continue;
        }
        #[cfg(feature = "test-support")]
        if let Some(installed) = config.ready_for_tests.get(&agent) {
            let version = installed.version.clone();
            worker.shared.emit(
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
        if paths_ok && let Some(installed) = receipt_install(config, &agent) {
            let version = installed.version.clone();
            worker.shared.emit(
                &agent,
                InstallState::Ready { version },
                Some(installed.clone()),
            );
            check.from_receipt.push((agent, installed));
            continue;
        }
        check.undecided.push(agent);
    }
    drop(lock);
    start(worker, check);
}

/// What the worker hands the check.
pub(super) struct Check {
    /// Agents sent `Ready` from their receipt, with the install that was sent.
    pub(super) from_receipt: Vec<(AgentId, Installed)>,
    /// Agents with no state yet: no receipt, or one naming a Node not on hand.
    pub(super) undecided: Vec<AgentId>,
    /// Whether the data folder and bundle paths are usable ([`install::check_paths`]).
    pub(super) paths_ok: bool,
}

/// `agent`'s install as its receipt says, from file reads and stats alone, with no child run:
/// the start sends `Ready` from it at once and checks the Node with
/// [`install::ready_install`] in the background. The receipt must be valid, and its Node a
/// regular file that is either the downloaded runtime's program (the runtime complete) or a
/// `node` the `PATH` search lists, as resolved: a receipt naming any other Node waits for the
/// check, which rewrites it, so no launch ever takes a Node this start would not consider.
pub(super) fn receipt_install(config: &AgentsConfig, agent: &AgentId) -> Option<Installed> {
    let entry = config.snapshot.agent(agent)?;
    let hash = config.snapshot.install_hash(agent, config.platform)?;
    let folder = receipt::install_folder(&config.data_dir, agent, &hash);
    let receipt = receipt::read_receipt(&folder, agent, &hash, config.platform)?;
    if !std::fs::metadata(&receipt.node).is_ok_and(|meta| meta.is_file()) {
        return None;
    }
    let runtime = node::runtime_dir(&config.data_dir, config.snapshot.node(), config.platform);
    let runtime = node::runtime_program(&runtime, config.platform.os);
    let known = if receipt.node == runtime {
        install::runtime_ready(config)
    } else {
        node::candidates(&config.node_search)
            .iter()
            .any(|candidate| dunce::canonicalize(candidate).is_ok_and(|path| path == receipt.node))
    };
    known.then(|| Installed {
        node: receipt.node.clone(),
        script: receipt::script_path(&folder, entry),
        folder,
        version: receipt.version,
    })
}

/// The check's own copy of what it needs.
struct Run<T> {
    config: AgentsConfig,
    launcher: Arc<dyn Launcher>,
    timer: Arc<T>,
    shared: Arc<Shared>,
    cancel: Arc<Cancel>,
}

/// Starts the check on its own thread, counted as a job so the quit waits for it. A thread
/// that cannot start leaves the check to run here, on the worker, as before.
pub(super) fn start<T: Timer + Send + Sync + 'static>(worker: &Worker<T>, check: Check) {
    let cancel = Arc::new(Cancel::new());
    worker.shared.lock().node_check.cancel = Some(Arc::clone(&cancel));
    let run = Arc::new(Run {
        config: worker.config.clone(),
        launcher: Arc::clone(&worker.launcher),
        timer: Arc::clone(&worker.timer),
        shared: Arc::clone(&worker.shared),
        cancel,
    });
    let check = Arc::new(std::sync::Mutex::new(Some(check)));
    let (on_thread, take) = (Arc::clone(&run), Arc::clone(&check));
    let spawned = worker
        .shared
        .spawn_job("clauseo-node-check".to_owned(), move || {
            if let Some(check) = take_check(&take) {
                on_thread.run(check);
            }
        });
    if spawned.is_err()
        && let Some(check) = take_check(&check)
    {
        run.run(check);
    }
}

fn take_check(check: &std::sync::Mutex<Option<Check>>) -> Option<Check> {
    check
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

impl<T: Timer> Run<T> {
    fn run(&self, check: Check) {
        let work = pin!(self.work(check));
        // Either the work ended or the stop cancelled it; dropping the work kills any child.
        futures::executor::block_on(select(work, pin!(self.cancel.cancelled())));
        let mut inner = self.shared.lock();
        if inner
            .node_check
            .cancel
            .as_ref()
            .is_some_and(|ours| Arc::ptr_eq(ours, &self.cancel))
        {
            inner.node_check.cancel = None;
        }
    }

    async fn work(&self, check: Check) {
        let (config, launcher, timer) = (&self.config, &*self.launcher, &*self.timer);
        let search = node::search_user_node(&config.node_search, launcher, timer).await;
        for agent in check.undecided {
            if self.shared.lock().states.contains_key(&agent) {
                // An install started meanwhile: it reports for itself.
                continue;
            }
            let ready = if check.paths_ok {
                install::ready_install(config, launcher, timer, &agent, &search).await
            } else {
                None
            };
            let (state, installed) = if let Some(installed) = ready {
                let version = installed.version.clone();
                (InstallState::Ready { version }, Some(installed))
            } else {
                let node_needed = search.found.is_none() && !install::runtime_ready(config);
                let download_bytes = config
                    .snapshot
                    .download_bytes(&agent, config.platform, node_needed)
                    .unwrap_or(0);
                (InstallState::Missing { download_bytes }, None)
            };
            self.set_if(
                &agent,
                |inner| !inner.states.contains_key(&agent),
                state,
                installed,
            );
        }
        // After every first state: the receipt agents have theirs already.
        for (agent, sent) in check.from_receipt {
            let checked = install::ready_install(config, launcher, timer, &agent, &search).await;
            self.checked(&agent, &sent, checked);
        }
    }

    /// A receipt agent's check is done: `checked` is what [`install::ready_install`] found.
    fn checked(&self, agent: &AgentId, sent: &Installed, checked: Option<Installed>) {
        let (state, installed) = match checked {
            Some(installed) if installed == *sent => return,
            Some(installed) => {
                let version = installed.version.clone();
                (InstallState::Ready { version }, Some(installed))
            }
            None => {
                tracing::warn!(%agent, "agent start: the receipt's Node failed its check; the agent is failed until repaired");
                let mut log = InstallLog::new(agent);
                log.push(FAILED_LINE.to_owned());
                log.push(format!(
                    "result: {} after 0 tries",
                    FailureKind::SetupFailed.word()
                ));
                let failure = Failure {
                    kind: FailureKind::SetupFailed,
                    need_mb: None,
                    free_mb: None,
                    log,
                };
                (InstallState::Failed(failure), None)
            }
        };
        let still_sent = |inner: &Inner| {
            matches!(inner.states.get(agent), Some(InstallState::Ready { .. }))
                && inner.installed.get(agent) == Some(sent)
        };
        self.set_if(agent, still_sent, state, installed);
    }

    /// Records and sends `state` when `holds` (under the lock) and the check is not cancelled.
    /// A `Failed` is kept for [`repair_for_sign_in`] under the same lock, so a press that sees
    /// it repairs it.
    fn set_if(
        &self,
        agent: &AgentId,
        holds: impl FnOnce(&Inner) -> bool,
        state: InstallState,
        installed: Option<Installed>,
    ) {
        let mut inner = self.shared.lock();
        if self.cancel.is_cancelled() || !holds(&inner) {
            return;
        }
        if matches!(state, InstallState::Failed(_)) {
            inner.node_check.failed.insert(agent.clone(), state.clone());
        }
        let ready = matches!(state, InstallState::Ready { .. });
        Inner::set(&mut inner, &self.shared.events, agent, state, installed);
        drop(inner);
        // After the lock: a queued sign-in reads the install again once kicked.
        if ready {
            self.shared.signin.install_ready(agent);
        }
    }
}
