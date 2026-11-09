//! The worker's first states at start: each snapshot agent's install state, and the Python
//! runtime's, after the stale folders are removed under `agents/.lock`.

use clauseo_process::Timer;
use clauseo_protocol::AgentId;

use super::Worker;
use crate::state::InstallState;
use crate::{install, node, python, receipt};

/// Each snapshot agent's state at start, then Python's.
pub(super) async fn first_states<T: Timer + Send + Sync + 'static>(worker: &Worker<T>) {
    let config = &worker.config;
    let search =
        node::search_user_node(&config.node_search, &*worker.launcher, &*worker.timer).await;
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
        // An app update's new Python archive leaves the old runtime (read-only on Unix)
        // behind; what will not go now goes at the next start.
        let keep = config.snapshot.python_archive(config.platform);
        let keep = keep.map(crate::snapshot::PythonArchive::stem);
        drop(python::remove_stale_runtimes(&config.data_dir, keep));
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
                install::ready_install(config, &*worker.launcher, &*worker.timer, &agent, &search)
                    .await
        {
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
