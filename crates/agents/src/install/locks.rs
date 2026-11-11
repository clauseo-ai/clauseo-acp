//! The lock files one install job holds in `agents/` ([`write::AgentsLock`]), and the one wait
//! for a lock: `.lock-node` while another try fetches or unpacks Node.
//!
//! Each lock is a file lock (`flock` on Unix, `LockFileEx` on Windows) on its own open of the
//! file, so it excludes another job of this process as it excludes another process, and the OS
//! gives it up when the file is closed: a job that ends any way, a crash included, leaves no
//! lock behind. A lock file is never removed (removing one another process holds open would
//! let a third take a lock on a new file of the same name).

use std::fs::File;
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use clauseo_process::Timer;

use super::{Job, TryError};
use crate::config::AgentsConfig;
use crate::write::{self, AgentsLock, Mode};

/// How often a try waiting for a lock tries it again.
pub(crate) const LOCK_POLL: Duration = Duration::from_millis(150);
/// How many times it tries before the try fails: 15 minutes, past a slow Node download of
/// another try (its archive is about 30 to 60 MB) and its unpack under an antivirus.
const LOCK_POLLS: u32 = 6000;

/// The locks one job holds, each given up before the job's last event.
#[derive(Debug, Default)]
pub(crate) struct Locks {
    /// `.lock` (shared) and the agent's `.lock-<id>` (exclusive), for the whole job.
    job: Mutex<Vec<File>>,
    /// `.lock-node`: exclusive while this try checks, fetches and unpacks Node, shared while
    /// its npm runs from the runtime; `None` otherwise.
    node: Mutex<Option<File>>,
    /// `.lock-python`, while this job installs Python.
    python: Mutex<Option<File>>,
}

impl Locks {
    /// Keeps the job's `.lock` and `.lock-<id>`.
    pub(super) fn hold_job(&self, all: File, own: File) {
        *guard(&self.job) = vec![all, own];
    }

    /// Keeps `.lock-node`, in whichever mode it was taken.
    pub(super) fn hold_node(&self, file: File) {
        *guard(&self.node) = Some(file);
    }

    /// Gives up `.lock-node`, if held.
    pub(crate) fn release_node(&self) {
        drop(guard(&self.node).take());
    }

    /// Keeps `.lock-python`.
    pub(super) fn hold_python(&self, file: File) {
        *guard(&self.python) = Some(file);
    }

    /// Gives up every lock, Node's and Python's first.
    pub(super) fn release_all(&self) {
        self.release_node();
        drop(guard(&self.python).take());
        drop(std::mem::take(&mut *guard(&self.job)));
    }
}

fn guard<L>(lock: &Mutex<L>) -> MutexGuard<'_, L> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Takes `lock` in `mode` for `job`, trying again every [`LOCK_POLL`] while another holder's
/// conflicts, and hands it to `keep`. The wait is raced against the job's pause (a cancelled
/// try's error) and fails the try past [`LOCK_POLLS`] tries; a lock that cannot be opened fails
/// it at once.
pub(super) async fn wait_for<T: Timer + Send + Sync>(
    job: &Job<T>,
    lock: AgentsLock<'_>,
    mode: Mode,
    keep: impl FnOnce(File),
) -> Result<(), TryError> {
    let config = &job.config;
    let mut waited = false;
    for _ in 0..LOCK_POLLS {
        match write::try_lock(&config.data_dir, lock, mode) {
            Ok(Some(file)) => {
                if waited {
                    tracing::info!(agent = %job.ticket.agent, ?lock, "agent install: lock taken");
                }
                keep(file);
                return Ok(());
            }
            Ok(None) => {}
            // An antivirus scanning the lock file holds it a moment (Windows' 5, 32, 33).
            Err(err) if super::npm::windows_busy(&err, config.platform.os).is_some() => {}
            Err(err) => return Err(TryError::io("locking the agents folder", &err, config)),
        }
        if !waited {
            waited = true;
            tracing::info!(
                agent = %job.ticket.agent,
                ?lock,
                "agent install: waiting for another install's lock"
            );
        }
        if !job.wait(LOCK_POLL).await {
            return Err(TryError::cancelled());
        }
    }
    Err(TryError::retry(
        "another install held the downloaded Node for 15 minutes",
        config,
    ))
}

/// Removes the downloaded runtime at `dir`, which npm could not start from or load its modules
/// from: only with `.lock-node` taken exclusive, after this try's shared hold (in `locks`) is
/// given up, so no npm of another try runs from it. While another holds it the runtime is kept
/// (that npm may be fine), and this try's next one checks it again. The words for the log.
pub(super) fn remove_runtime(locks: &Locks, config: &AgentsConfig, dir: &Path) -> &'static str {
    locks.release_node();
    match write::try_lock(&config.data_dir, AgentsLock::Node, Mode::Exclusive) {
        Ok(Some(held)) => {
            drop(write::remove_dir_all(dir));
            drop(held);
            "; the downloaded Node is removed and will be fetched again"
        }
        _ => "; the downloaded Node is kept: another install is using it",
    }
}
