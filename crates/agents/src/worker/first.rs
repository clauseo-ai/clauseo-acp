//! The worker's first states at start: each snapshot agent's install state, and the Python
//! runtime's, after the stale folders are removed, each under its lock in `agents/`: an
//! agent's under its `.lock-<id>`, Python's under `.lock-python`, and what an older Clauseo
//! left in the shared places under `.lock` (all of it with `.lock` held shared). A lock another
//! process holds skips its part of the clean-up: that Clauseo may be installing it now.

use clauseo_process::Timer;
use clauseo_protocol::AgentId;

use super::Worker;
use crate::state::InstallState;
use crate::write::{self, AgentsLock, Mode};
use crate::{install, node, python, receipt};

/// Each snapshot agent's state at start, then Python's.
pub(super) async fn first_states<T: Timer + Send + Sync + 'static>(worker: &Worker<T>) {
    let config = &worker.config;
    let search =
        node::search_user_node(&config.node_search, &*worker.launcher, &*worker.timer).await;
    let agents: Vec<AgentId> = config.snapshot.agents().map(|e| e.id.clone()).collect();
    // The clean-up below runs only with its locks held: another Clauseo installing now may
    // own the temp folders it would remove. A relative data folder is never touched.
    let paths_ok = install::check_paths(config).is_ok();
    let take = |lock: AgentsLock<'_>, mode: Mode| {
        paths_ok
            .then(|| write::try_lock(&config.data_dir, lock, mode).ok().flatten())
            .flatten()
    };
    let lock = take(AgentsLock::All, Mode::Shared);
    if lock.is_some() {
        // No older Clauseo is installing: what it left in the shared npm cache and downloads.
        install::remove_legacy_shared(&config.data_dir, &agents);
        if let Some(python) = take(AgentsLock::Python, Mode::Exclusive) {
            // An app update's new Python archive leaves the old runtime (read-only on Unix)
            // behind; what will not go now goes at the next start.
            let keep = config.snapshot.python_archive(config.platform);
            let keep = keep.map(crate::snapshot::PythonArchive::stem);
            drop(python::remove_stale_runtimes(&config.data_dir, keep));
            drop(python);
        }
    }
    worker.shared.set_python_bytes(
        config
            .snapshot
            .python_archive(config.platform)
            .map_or(0, crate::snapshot::PythonArchive::size),
    );
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
        let own = lock
            .as_ref()
            .and_then(|_| take(AgentsLock::Agent(&agent), Mode::Exclusive));
        if own.is_some() {
            // What a try cut short by the last quit left in the agent's npm cache.
            install::empty_npm_cache(&config.data_dir, &agent);
        }
        if own.is_some()
            && let Some(hash) = config.snapshot.install_hash(&agent, config.platform)
        {
            let scan = receipt::scan(&config.data_dir, &agent, &hash, config.platform);
            // A folder that will not go now goes at the next start, or under the install's
            // own rename.
            drop(scan.and_then(|scan| receipt::remove_stale(&scan)));
        }
        // `own` is held to the end of this turn: a receipt rewritten below is under it.
        if paths_ok
            && let Some(installed) =
                install::ready_install(config, &*worker.launcher, &*worker.timer, &agent, &search)
                    .await
        {
            // Codex's unsigned rg.exe moves off PATH under Smart App Control, or back once it
            // is off (Windows; never fails). The receipt vouches for the folder either way.
            crate::sac::settle(config, &installed.folder, &agent);
            let version = installed.version.clone();
            worker
                .shared
                .emit(&agent, InstallState::Ready { version }, Some(installed));
            continue;
        }
        let node_needed = search.found.is_none() && !install::runtime_ready(config);
        let download_bytes = config
            .snapshot
            .download_bytes(&agent, config.platform, node_needed)
            .unwrap_or(0);
        worker
            .shared
            .emit(&agent, InstallState::Missing { download_bytes }, None);
    }
    drop(lock);
    worker.shared.set_python(python::resting_state(config));
}
