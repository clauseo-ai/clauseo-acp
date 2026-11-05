//! The host's start, off the UI thread: setup opens the window first and starts the host on a
//! thread of its own (`Host::start` blocks until the store opens), so the webview does not wait
//! for the store. The [`HostCell`] is where the start's outcome lands. Every bridge call awaits
//! it ([`HostCell::host`]): one sent before the host is ready waits, never fails, and after a
//! failed start every call gets that start's error (`store.newer_version`, `store.corrupt`, ...).
//! Exit waits for the start too, then stops the host ([`stop`]).

use std::sync::{Arc, Mutex, OnceLock};

use clauseo_host::{HostExit, HostHandle, HostJoin};
use clauseo_protocol::{ErrorCode, HostError};
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
/// `clauseo-host-start`, its outcome going into `start`'s cell.
pub(crate) fn start_on_thread(
    start: HostStart,
    run: impl FnOnce() -> Result<(HostHandle, HostJoin), HostError> + Send + 'static,
) -> std::io::Result<()> {
    let thread = std::thread::Builder::new()
        .name("clauseo-host-start".to_owned())
        .spawn(move || start.finish(run()))?;
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

#[cfg(test)]
mod tests;
