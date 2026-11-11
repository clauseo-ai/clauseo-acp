//! The finder, bounded: it runs on a worker thread of its own, its caller waits at most
//! [`TIMEOUT`] for it, and while a worker still runs (a read that hangs, as on a network
//! folder) no other starts. A scan and an import each have their own flag, so a slow scan never
//! fails an import; at most one worker of each kind runs at a time.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clauseo_settings::mcp_import::ImportCandidate;

use crate::config::mcp::McpFinder;

/// How long a scan or an import waits for the finder.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);

/// The name of a finder's worker thread.
const WORKER_THREAD: &str = "clauseo-mcp-find";

/// What `find` found for `existing`, or `None` when it took longer than `timeout`, panicked,
/// its thread did not start, or the worker before it (`busy`) still runs.
pub(crate) fn bounded(
    find: &McpFinder,
    existing: Vec<String>,
    busy: &Arc<AtomicBool>,
    timeout: Duration,
) -> Option<Vec<ImportCandidate>> {
    if busy.swap(true, Ordering::AcqRel) {
        tracing::warn!("the MCP import finder before still runs; this one does not start");
        return None;
    }
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let find = Arc::clone(find);
    let clear = Clear(Arc::clone(busy));
    let started = std::thread::Builder::new()
        .name(WORKER_THREAD.to_owned())
        .spawn(move || {
            let _clear = clear;
            let _gone = send.send(find(&existing));
        });
    if let Err(error) = started {
        tracing::warn!(%error, "the MCP import finder did not start");
        return None;
    }
    let found = receive.recv_timeout(timeout).ok();
    if found.is_none() {
        tracing::warn!("the MCP import finder did not answer in time");
    }
    found
}

/// Clears the busy flag when the worker ends, panicking or not, or when it never started.
struct Clear(Arc<AtomicBool>);

impl Drop for Clear {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "finder_tests.rs"]
mod tests;
