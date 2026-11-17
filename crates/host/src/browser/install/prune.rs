//! Old copies of Chrome for Testing, and which sessions run from them (decision record 0029,
//! "Never older, never stale"). A copy goes only once a newer one is in place and no session
//! runs from it.
//!
//! The launch prep notes each session that got a downloaded copy ([`InUse::note`]) with its
//! run folder; the note counts while that folder exists (`launch_checks::discard` removes it
//! once the agent's tree was killed, and the run sweep sets an earlier host's aside). The prune
//! and the launch prep's search share one lock ([`InUse::search`]), so no session can pick a
//! copy between the prune's look and its removal.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use super::fresh;
use crate::browser::chrome;
use crate::write;

/// The sessions running a downloaded copy: each one's run folder and the copy's folder
/// (`browsers/chrome-<version>`). Shared by the actor's download and every launch prep.
#[derive(Debug, Clone, Default)]
pub struct InUse(Arc<Mutex<Vec<(PathBuf, PathBuf)>>>);

impl InUse {
    /// None noted yet.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Vec<(PathBuf, PathBuf)>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Runs `search` (the launch prep's Chrome search) under the lock, and notes the session
    /// whose run folder is `run_root` against the copy it found, when that is a downloaded one
    /// under `browsers`.
    pub fn search(
        &self,
        browsers: &Path,
        run_root: &Path,
        search: impl FnOnce() -> Option<PathBuf>,
    ) -> Option<PathBuf> {
        let mut noted = self.lock();
        let found = search()?;
        if let Some(folder) = copy_folder(browsers, &found) {
            noted.push((run_root.to_owned(), folder));
        }
        Some(found)
    }

    /// Notes that the session whose run folder is `run_root` runs the copy in `folder`.
    pub fn note(&self, run_root: &Path, folder: &Path) {
        self.lock().push((run_root.to_owned(), folder.to_owned()));
    }

    /// Whether a session still runs from `folder`: a note for it whose run folder is there.
    /// Notes whose run folder is gone are dropped.
    fn held(noted: &mut Vec<(PathBuf, PathBuf)>, folder: &Path) -> bool {
        noted.retain(|(run_root, _)| std::fs::symlink_metadata(run_root).is_ok());
        noted.iter().any(|(_, used)| used == folder)
    }
}

/// The copy's folder (`browsers/chrome-<version>`) that `binary` sits in, when it sits in one.
fn copy_folder(browsers: &Path, binary: &Path) -> Option<PathBuf> {
    let rest = binary.strip_prefix(browsers).ok()?;
    let first = rest.components().next()?;
    Some(browsers.join(first))
}

/// Removes every copy in `browsers` older than the highest working one (whose `binary` is
/// there), but those a session still runs from. Returns the folders removed. A removal that
/// fails is logged and left for the next. Nothing goes while no copy works.
pub fn prune(browsers: &Path, binary: &Path, in_use: &InUse) -> Vec<PathBuf> {
    let mut noted = in_use.lock();
    let Some(newest) = fresh::working(browsers, binary)
        .pop()
        .and_then(|(version, _)| chrome::version(&version))
    else {
        return Vec::new();
    };
    let older = fresh::installed(browsers)
        .into_iter()
        .filter(|(version, _)| chrome::version(version).is_some_and(|v| v < newest));
    let mut removed = Vec::new();
    for (version, folder) in older {
        if InUse::held(&mut noted, &folder) {
            tracing::info!(%version, "an old Chrome for Testing stays: a session runs from it");
            continue;
        }
        match write::remove_tree(&folder) {
            Ok(()) => {
                tracing::info!(%version, "an old Chrome for Testing was removed");
                removed.push(folder);
            }
            Err(error) => tracing::warn!(
                %version,
                error = %crate::log_file::Cause(&error),
                "an old Chrome for Testing was not removed"
            ),
        }
    }
    removed
}
