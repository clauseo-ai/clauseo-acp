//! The host's start, off the UI thread: setup opens the window first and starts the host on a
//! thread of its own (`Host::start` blocks until the store opens), so the webview does not wait
//! for the store. The [`HostCell`] is where the start's outcome lands. Every bridge call awaits
//! it ([`HostCell::host`]): one sent before the host is ready waits, never fails, and after a
//! failed start every call gets that start's error (`store.newer_version`, `store.corrupt`, ...).
//! Exit waits for the start too, then stops the host ([`stop`]).

use std::sync::{Arc, Mutex, OnceLock};

use clauseo_host::{HostConfig, HostExit, HostHandle, HostJoin};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_protocol::{ErrorCode, HostError};
use clauseo_settings::{Effective, Locations, PlatformDirs as _, SystemDirs, SystemFiles};
use futures::channel::oneshot;
use futures::future::{FutureExt as _, Shared};

use crate::bridge::lock;

/// Where the host's start lands: its handle and thread, or why it failed.
pub(crate) struct HostCell {
    /// The start's outcome, set once.
    started: OnceLock<Result<HostHandle, HostError>>,
    /// The host thread, until exit takes it to join it.
    join: Mutex<Option<HostJoin>>,
    /// Resolves once `started` is set, or the start was dropped without an outcome.
    done: Shared<oneshot::Receiver<()>>,
}

/// The message of the `internal` error a start dropped without an outcome leaves.
const NO_OUTCOME: &str = "the host's start ended without an outcome";

/// The one way to fill a [`HostCell`]. Dropped without [`finish`](HostStart::finish) (the
/// starting thread unwound, in a dev or test build: release aborts on panic; or setup failed
/// after `HostCell::new`), every waiter gets `internal`.
pub(crate) struct HostStart {
    cell: Arc<HostCell>,
    done: oneshot::Sender<()>,
}

impl HostCell {
    /// An empty cell, and the start that fills it.
    pub(crate) fn new() -> (Arc<Self>, HostStart) {
        let (done, wait) = oneshot::channel();
        let cell = Arc::new(Self {
            started: OnceLock::new(),
            join: Mutex::new(None),
            done: wait.shared(),
        });
        let start = HostStart {
            cell: Arc::clone(&cell),
            done,
        };
        (cell, start)
    }

    /// A cell already holding `host`, whose thread someone else joins.
    #[cfg(test)]
    pub(crate) fn ready(host: HostHandle) -> Arc<Self> {
        let (cell, start) = Self::new();
        start.finish_with(Ok(host), None);
        cell
    }

    /// The host, once its start finished; the start's error when it failed.
    pub(crate) async fn host(&self) -> Result<&HostHandle, HostError> {
        // Canceled (the start was dropped) leaves `started` unset: `internal` below.
        let _finished = self.done.clone().await;
        match self.started.get() {
            Some(Ok(host)) => Ok(host),
            Some(Err(error)) => Err(error.clone()),
            None => Err(HostError::new(ErrorCode::Internal, NO_OUTCOME)),
        }
    }
}

impl HostStart {
    /// Puts the outcome of `Host::start` in the cell and wakes every waiter.
    pub(crate) fn finish(self, started: Result<(HostHandle, HostJoin), HostError>) {
        match started {
            Ok((host, join)) => self.finish_with(Ok(host), Some(join)),
            Err(error) => self.finish_with(Err(error), None),
        }
    }

    fn finish_with(self, started: Result<HostHandle, HostError>, join: Option<HostJoin>) {
        // The thread first: a waiter woken below finds it.
        *lock(&self.cell.join) = join;
        // The only setter, and `self` is consumed: the cell is empty here.
        let _set = self.cell.started.set(started);
        // No receiver left only when the cell itself is gone.
        let _woken = self.done.send(());
    }
}

/// Runs `run` (in the app, `Host::start` with the effective settings) on a thread named
/// `clauseo-host-start`, its outcome going into `start`'s cell. Logs when it ended, by the
/// chooser's clock (`chooser/trace.rs`): until then every bridge call waits.
pub(crate) fn start_on_thread(
    start: HostStart,
    run: impl FnOnce() -> Result<(HostHandle, HostJoin), HostError> + Send + 'static,
) -> std::io::Result<()> {
    let thread = std::thread::Builder::new()
        .name("clauseo-host-start".to_owned())
        .spawn(move || {
            let began = crate::open::now();
            // Below normal on Windows until the start ends, this thread and the start's others
            // (start_priority.rs); the guard restores them however `run` ends.
            let restore = crate::start_priority::restore_on_drop();
            crate::start_priority::lower_current("clauseo-host-start");
            let started = run();
            let how = if started.is_ok() { "started" } else { "failed" };
            crate::chooser::trace::mark("host start ended", began, how);
            drop(restore);
            start.finish(started);
        })?;
    // Not joined: the cell carries what it did.
    drop(thread);
    Ok(())
}

/// Stops the host, blocking: waits for its start, shuts it down and joins its thread, and says
/// how it ended: `HostExit::Install` when it had already stopped to install an update (0037).
/// After a failed start there is nothing to stop: `HostExit::Stopped`.
pub(crate) fn stop(cell: &HostCell) -> HostExit {
    let host = futures::executor::block_on(cell.host()).ok();
    if let Some(host) = host {
        // Already stopped or failed: join reports it, and the app exits either way.
        let _stopped = futures::executor::block_on(host.shutdown());
    }
    let join = lock(&cell.join).take();
    if let Some(join) = join {
        let _joined = join.join();
    }
    // Resolved once the thread has ended, which the join above waited for.
    host.map_or(HostExit::Stopped, |host| {
        futures::executor::block_on(host.exit())
    })
}

/// Starts `lookup` (`Locations::with_documents` with the system's folders, or a test's stand-in)
/// on a thread of its own and gives the receiver its answer comes on, for
/// `HostConfig::with_documents_later`: the host's start never waits on Documents, which on
/// Windows may wait 2 s on a share that is down. Marks `began`'s timing line when it answers.
/// With no thread the sender is dropped, and the host goes on without Documents.
pub(crate) fn documents_later(
    locations: Locations,
    began: std::time::Instant,
    lookup: impl FnOnce(Locations) -> Locations + Send + 'static,
) -> oneshot::Receiver<Locations> {
    let (answer, later) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("clauseo-documents".into())
        .spawn(move || {
            crate::start_priority::lower_current("clauseo-documents");
            let located = lookup(locations);
            let found = if located.default_matters_dir().is_some() {
                "found"
            } else {
                "none"
            };
            crate::chooser::trace::mark("host start: Documents looked up", began, found);
            // The host may be gone (a failed start, a quit); then nobody is left to tell.
            drop(answer.send(located));
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "no Documents lookup thread: no default matters folder this run");
    }
    later
}

/// The launcher the host starts every agent and plan reader with: it clears the environment
/// (`SpawnOptions::clear_env`, as `HostConfig::new` requires), so a child gets its spec's
/// variables and nothing else, and the variables the host drops (`GIT_*`, `SSH_*`, Codex's
/// dropped names) stay dropped. `home` shows as `~` in stderr tails.
pub(crate) fn host_launcher(home: Option<std::path::PathBuf>) -> PassthroughLauncher {
    PassthroughLauncher::new(host_spawn_options(home))
}

/// [`host_launcher`]'s options: the environment cleared, `home` shown as `~`. S18's sign-in
/// launcher uses them too (`setup/start.rs`).
pub(crate) fn host_spawn_options(home: Option<std::path::PathBuf>) -> SpawnOptions {
    let spawn = SpawnOptions::new().clear_env();
    match home {
        Some(home) => spawn.home(home),
        None => spawn,
    }
}

/// The effective settings under `locations`, with the system policy sources.
pub(crate) fn load_effective(locations: &Locations) -> Effective {
    let sources = clauseo_settings::system_policy_sources();
    let sources: Vec<&dyn clauseo_settings::PolicySource> =
        sources.iter().map(AsRef::as_ref).collect();
    Effective::load(locations, &SystemFiles, &sources)
}

/// The host's config: the effective settings, the launcher that clears the environment
/// (`crate::host_launcher`), and the refused folders.
pub(crate) fn host_config(effective: Effective) -> HostConfig {
    let launcher = crate::host_launcher(SystemDirs.home());
    let config = HostConfig::new(effective, Arc::new(launcher))
        .with_refused_folders(clauseo_settings::refused_folders(&SystemDirs));
    // With no path to this executable no guard hook goes; the permission check remains.
    match std::env::current_exe() {
        Ok(exe) => config.with_pre_write_guard(exe),
        Err(_) => config,
    }
}

#[cfg(test)]
mod tests;
