//! A folder lookup with a time limit, for a known folder that may sit on a network share: a
//! Documents folder redirected to a share that is down (a firm laptop off VPN) can hold an
//! existence check on SMB for 20 to 40 s or more. The desktop looks Documents up on a thread of
//! its own (`Locations::with_documents`), never the main one or the host's start, and the host
//! takes it when it answers. Plain Rust, so it is tested on every OS.

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, mpsc};
use std::time::Duration;

/// How long the Documents lookup waits for the folder before it goes on without one.
pub(crate) const DOCUMENTS_WAIT: Duration = Duration::from_secs(2);

/// Why a looked-up folder is not used. Logged; never carries the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unavailable {
    /// The lookup gave no folder, or the folder is not there (or not a folder).
    Missing,
    /// The lookup or the check did not answer within the wait.
    TimedOut,
}

/// The folder `lookup` names, if `is_dir` says it is a folder, checked once for `cell`: both
/// run on a thread of their own, given at most `wait`. A lookup that runs out of time is left to
/// finish on that thread, detached: nothing joins it, and it holds no lock, only the sending end
/// of a channel whose receiver is gone by then. The first caller runs the check; a caller that
/// comes during its wait blocks on `get_or_init` until it ends (at most `wait`), and later
/// callers get its answer at once. The cell's lock is held only for the wait, never by the
/// detached thread, so nothing on start or quit can block on a lookup stuck on the network.
///
/// Each call that gives no folder warns with the reason, from the cached answer: the desktop's
/// first call comes before its log file is open, so the warning reaches the log from a later
/// one. `name` names the folder in it; it never has the path (log rule: no user paths).
pub(crate) fn verified_dir_once(
    cell: &OnceLock<Result<PathBuf, Unavailable>>,
    name: &'static str,
    wait: Duration,
    lookup: impl FnOnce() -> Option<PathBuf> + Send + 'static,
    is_dir: impl FnOnce(&Path) -> bool + Send + 'static,
) -> Option<PathBuf> {
    match cell.get_or_init(|| check(wait, lookup, is_dir)) {
        Ok(folder) => Some(folder.clone()),
        Err(reason) => {
            let reason = match reason {
                Unavailable::Missing => "missing",
                Unavailable::TimedOut => "timed out",
            };
            tracing::warn!(
                folder = name,
                reason,
                "folder unavailable; going on without it"
            );
            None
        }
    }
}

/// The check of [`verified_dir_once`], uncached and without the warning.
fn check(
    wait: Duration,
    lookup: impl FnOnce() -> Option<PathBuf> + Send + 'static,
    is_dir: impl FnOnce(&Path) -> bool + Send + 'static,
) -> Result<PathBuf, Unavailable> {
    let (send, answer) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("clauseo-folder-check".into())
        .spawn(move || {
            let found = lookup().filter(|folder| is_dir(folder));
            // The receiver is gone when the wait ran out; the answer is then dropped.
            drop(send.send(found));
        });
    if spawned.is_err() {
        // No thread to bound the check with: going on without the folder beats a hang.
        return Err(Unavailable::TimedOut);
    }
    // The handle is dropped here: a lookup still stuck on the network finishes on its own.
    match answer.recv_timeout(wait) {
        Ok(Some(folder)) => Ok(folder),
        Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => Err(Unavailable::Missing),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Unavailable::TimedOut),
    }
}
