//! The open matter's folder, watched (decision record 0055): the OS's own change feed through
//! the `notify` crate (`FSEvents` on macOS, `ReadDirectoryChangesW` on Windows, inotify on Linux),
//! the whole tree as the listing walks it, never a polling loop. No file is opened: inotify and
//! `FSEvents` hold no handle at all, and on Windows the one handle is the folder's own, opened
//! with every share mode, so Word saves, renames and deletes in it as before.
//!
//! A change to a file the listing never shows (`~$` owner files, `*.tmp`, `.clauseo`, `.git`:
//! `clauseo_matters::is_excluded_path`) and a read (an access event: the tracked-changes scan
//! opens `.docx` files) are passed over. The rest wake a `clauseo-papers` thread, which waits
//! until the folder has been quiet for the debounce (a Word save is temporary files and a rename
//! in a few milliseconds) or [`BURST_CAP`] changes have come, then stats the folder and posts
//! [`FolderChanged`]. Dropping the [`Watch`] stops the OS's feed, which drops the sender, and
//! the thread ends.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use clauseo_protocol::MatterId;
use notify::event::{EventKind, MetadataKind, ModifyKind};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::actor::input::Input;

/// How long the folder must stay quiet before its papers are listed again: a Word save's
/// temporary files and rename land as one change.
const PAPERS_DEBOUNCE: Duration = Duration::from_millis(300);

/// The changes one burst may hold before it is told anyway, so a folder that never goes quiet (a
/// sync client bringing down a large tree) still updates as it fills.
const BURST_CAP: usize = 256;

/// The name of the thread that waits out each burst.
const THREAD_NAME: &str = "clauseo-papers";

/// What the thread posts: `matter`'s `folder` changed, and whether it is still a folder.
#[derive(Debug)]
pub(crate) struct FolderChanged {
    pub(crate) matter: MatterId,
    pub(crate) folder: PathBuf,
    pub(crate) found: bool,
}

/// One folder's watch; dropping it ends it.
pub(crate) struct Watch {
    pub(crate) folder: PathBuf,
    _watcher: RecommendedWatcher,
}

impl Watch {
    /// Watches `folder` of `matter`, posting each quiet change to `post`.
    pub(crate) fn start(
        matter: MatterId,
        folder: PathBuf,
        post: async_channel::Sender<Input>,
    ) -> Result<Self, notify::Error> {
        let (send, changes) = mpsc::channel::<()>();
        // `FSEvents` names paths as resolved (`/private/var` for `/var`): both spellings count.
        let roots: Vec<PathBuf> = std::iter::once(folder.clone())
            .chain(dunce::canonicalize(&folder).ok())
            .collect();
        let handler = move |event: notify::Result<notify::Event>| {
            if shows(&roots, &event) {
                // The thread is gone only once the watch is being dropped.
                let _gone = send.send(());
            }
        };
        let config = Config::default().with_follow_symlinks(false);
        let mut watcher = RecommendedWatcher::new(handler, config)?;
        watcher.watch(&folder, RecursiveMode::Recursive)?;
        let stat = folder.clone();
        std::thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || bursts(matter, &stat, PAPERS_DEBOUNCE, &changes, &post))
            .map_err(notify::Error::io)?;
        Ok(Self {
            folder,
            _watcher: watcher,
        })
    }
}

/// Whether `event` may change what the listing of the folder at `roots` shows.
fn shows(roots: &[PathBuf], event: &notify::Result<notify::Event>) -> bool {
    let Ok(event) = event else {
        // The feed lost track (an overflow): list again.
        return true;
    };
    if matches!(
        event.kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
    ) {
        return false;
    }
    event.paths.is_empty() || event.paths.iter().any(|path| listed(roots, path))
}

/// Whether `path` is the folder itself, or a file or folder the listing does not pass over.
fn listed(roots: &[PathBuf], path: &Path) -> bool {
    let Some(rel) = roots.iter().find_map(|root| path.strip_prefix(root).ok()) else {
        return true;
    };
    let mut parts = Vec::new();
    for part in rel.components() {
        match part.as_os_str().to_str() {
            Some(name) => parts.push(name),
            // Not a name the listing can show either way: list again.
            None => return true,
        }
    }
    parts.is_empty() || !clauseo_matters::is_excluded_path(&parts.join("/"))
}

/// Waits out each burst of `changes`, then posts what became of `folder`; ends when the watch
/// drops or the actor is gone.
fn bursts(
    matter: MatterId,
    folder: &Path,
    quiet: Duration,
    changes: &Receiver<()>,
    post: &async_channel::Sender<Input>,
) {
    while changes.recv().is_ok() {
        let mut seen = 1_usize;
        loop {
            match changes.recv_timeout(quiet) {
                Ok(()) => {
                    seen += 1;
                    if seen >= BURST_CAP {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        let changed = FolderChanged {
            matter,
            folder: folder.to_path_buf(),
            found: folder.is_dir(),
        };
        if post.try_send(Input::PapersChanged(changed)).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
