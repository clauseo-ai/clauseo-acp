//! The open matter's folder, watched (decision record 0055), on macOS and Linux only (Windows
//! builds `no_watch.rs` instead): the OS's own change feed through the `notify` crate (`FSEvents`
//! on macOS, inotify on Linux), the whole tree as the listing walks it, never a polling loop. No
//! file is opened, and neither feed holds a handle, so Word saves, renames and deletes as before.
//!
//! The watch starts on a short-lived `clauseo-watch-start` thread ([`start`]), never on the
//! actor: resolving the folder and inotify's walk of its tree would hold the host up on a share
//! that does not answer. The thread posts [`WatchStarted`] with the watch, or why there is none.
//!
//! A change to a file the listing never shows (`~$` owner files, `*.tmp`, `.clauseo`, `.git`:
//! `clauseo_matters::is_excluded_path`) and a read (an access event: the tracked-changes scan
//! opens `.docx` files) are passed over; a path the event does not say is a file or a folder
//! counts unless the listing passes it over as both. The rest wake a `clauseo-papers` thread,
//! which waits until the folder has been quiet for 300 ms (a Word save is temporary files and a
//! rename in a few milliseconds), or 2 s after the burst's first change when it never goes
//! quiet, then looks at the folder and posts [`FolderChanged`]. Dropping the [`Watch`] stops the
//! OS's feed, which drops the sender, and the thread ends.

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_protocol::{FolderState, MatterId};
use notify::event::{CreateKind, EventKind, MetadataKind, ModifyKind, RemoveKind};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::actor::folder_state::stat;
use crate::actor::input::Input;
use crate::clock::Clock;

/// How long the waits are: the real ones, or a test's.
#[derive(Debug, Clone, Copy)]
struct Timing {
    /// How long the folder must stay quiet before its papers are listed again: a Word save's
    /// temporary files and rename land as one change.
    quiet: Duration,
    /// The longest a burst waits after its first change, so a folder that never goes quiet (a
    /// sync client bringing a tree down) still updates as it fills.
    max_wait: Duration,
    /// How long after a folder first reads missing it is looked at again: a share that blinks
    /// reads missing for a moment.
    recheck: Duration,
}

const TIMING: Timing = Timing {
    quiet: Duration::from_millis(300),
    max_wait: Duration::from_secs(2),
    recheck: Duration::from_secs(2),
};

/// The name of the thread that waits out each burst.
const THREAD_NAME: &str = "clauseo-papers";

/// The name of the thread that starts a watch.
const START_THREAD: &str = "clauseo-watch-start";

/// A file name under a path, to ask whether the listing passes the path over as a folder.
const PROBE: &str = "paper.docx";

/// The folders whose start is still running (a share that does not answer can hold one for
/// long): a second start of one fails at once, so a hung folder strands at most one thread.
static STARTING: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn starting() -> std::sync::MutexGuard<'static, BTreeSet<PathBuf>> {
    STARTING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the burst thread posts: the watch of `generation` saw `matter`'s folder change.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FolderChanged {
    pub(crate) matter: MatterId,
    pub(crate) generation: u64,
    pub(crate) look: Look,
}

/// What became of the watched folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Look {
    /// Still the folder the watch started on.
    Found,
    /// Gone, or no longer a folder (twice, [`Timing::recheck`] apart).
    Missing,
    /// Another folder at the same path (a sync tool's swap): the watch is on the old one.
    Replaced,
}

/// What the start thread posts for the watch of `generation`.
pub(crate) struct WatchStarted {
    pub(crate) generation: u64,
    pub(crate) started: Started,
}

/// How a watch's start went.
pub(crate) enum Started {
    Live(Watch),
    /// The folder is not there.
    Missing,
    /// The OS refused (inotify's watch limit, an error from the share): logged by kind.
    Failed,
}

/// One folder's watch; dropping it ends it.
pub(crate) struct Watch {
    _watcher: RecommendedWatcher,
}

/// What one watch's burst thread knows.
struct Burst {
    matter: MatterId,
    generation: u64,
    folder: PathBuf,
    /// The folder's device and inode when the watch started.
    root: Option<(u64, u64)>,
    /// The host's clock, for the longest wait.
    clock: Arc<dyn Clock>,
}

/// Starts watching `folder` of `matter` on its own thread, which posts [`WatchStarted`] for
/// `generation` to `post`; each quiet change then comes as [`FolderChanged`].
pub(crate) fn start(
    matter: MatterId,
    folder: PathBuf,
    generation: u64,
    post: async_channel::Sender<Input>,
    clock: Arc<dyn Clock>,
) {
    let back = post.clone();
    let failed = WatchStarted {
        generation,
        started: Started::Failed,
    };
    if !starting().insert(folder.clone()) {
        let _gone = back.try_send(Input::WatchStarted(Box::new(failed)));
        return;
    }
    let held = folder.clone();
    let spawned = std::thread::Builder::new()
        .name(START_THREAD.to_owned())
        .spawn(move || {
            let started = begin(matter, &folder, generation, &post, clock);
            starting().remove(&folder);
            let started = WatchStarted {
                generation,
                started,
            };
            // The actor is gone only once the host has stopped.
            let _gone = post.try_send(Input::WatchStarted(Box::new(started)));
        });
    if let Err(error) = spawned {
        tracing::warn!(%matter, kind = ?error.kind(), "no thread to start the folder's watch");
        starting().remove(&held);
        let _gone = back.try_send(Input::WatchStarted(Box::new(failed)));
    }
}

fn begin(
    matter: MatterId,
    folder: &Path,
    generation: u64,
    post: &async_channel::Sender<Input>,
    clock: Arc<dyn Clock>,
) -> Started {
    match watch(matter, folder, generation, post, clock) {
        Ok(watcher) => Started::Live(Watch { _watcher: watcher }),
        Err(_) if stat(folder) == FolderState::Missing => Started::Missing,
        Err(error) => {
            tracing::info!(
                %matter,
                // The kind alone: the error's paths would put the folder in the log.
                kind = ?error.kind,
                "the matter's folder is not watched until it moves or another matter opens; \
                 its papers are listed again on focus"
            );
            Started::Failed
        }
    }
}

/// The OS's watch of `folder`, its changes waited out on a burst thread.
fn watch(
    matter: MatterId,
    folder: &Path,
    generation: u64,
    post: &async_channel::Sender<Input>,
    clock: Arc<dyn Clock>,
) -> Result<RecommendedWatcher, notify::Error> {
    let (send, changes) = mpsc::channel::<()>();
    // `FSEvents` names paths as resolved (`/private/var` for `/var`): both spellings count.
    let roots: Vec<PathBuf> = std::iter::once(folder.to_path_buf())
        .chain(dunce::canonicalize(folder).ok())
        .collect();
    let handler = move |event: notify::Result<notify::Event>| {
        if shows(&roots, &event) {
            // The thread is gone only once the watch is being dropped.
            let _gone = send.send(());
        }
    };
    let config = Config::default().with_follow_symlinks(false);
    let mut watcher = RecommendedWatcher::new(handler, config)?;
    let burst = Burst {
        matter,
        generation,
        folder: folder.to_path_buf(),
        root: root_id(folder),
        clock,
    };
    watcher.watch(folder, RecursiveMode::Recursive)?;
    let post = post.clone();
    std::thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || bursts(&burst, TIMING, &changes, &post))
        .map_err(notify::Error::io)?;
    Ok(watcher)
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
    let reading = Reading::of(event.kind);
    event.paths.is_empty() || event.paths.iter().any(|path| listed(roots, path, reading))
}

/// What an event says its path is.
#[derive(Debug, Clone, Copy)]
enum Reading {
    File,
    Folder,
    /// Either: a rename, or a feed that does not say.
    Either,
}

impl Reading {
    fn of(kind: EventKind) -> Self {
        match kind {
            EventKind::Create(CreateKind::File)
            | EventKind::Remove(RemoveKind::File)
            | EventKind::Modify(ModifyKind::Data(_)) => Self::File,
            EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder) => {
                Self::Folder
            }
            EventKind::Any
            | EventKind::Access(_)
            | EventKind::Create(_)
            | EventKind::Modify(_)
            | EventKind::Remove(_)
            | EventKind::Other => Self::Either,
        }
    }
}

/// Whether `path` is the folder itself, or a file or folder the listing does not pass over.
fn listed(roots: &[PathBuf], path: &Path, reading: Reading) -> bool {
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
    if parts.is_empty() {
        return true;
    }
    let rel = parts.join("/");
    let as_file = || clauseo_matters::is_excluded_path(&rel);
    let as_folder = || clauseo_matters::is_excluded_path(&format!("{rel}/{PROBE}"));
    let passed_over = match reading {
        Reading::File => as_file(),
        Reading::Folder => as_folder(),
        Reading::Either => as_file() && as_folder(),
    };
    !passed_over
}

/// Waits out each burst of `changes`, then posts what became of the folder; ends when the watch
/// drops or the actor is gone.
fn bursts(
    burst: &Burst,
    timing: Timing,
    changes: &Receiver<()>,
    post: &async_channel::Sender<Input>,
) {
    while changes.recv().is_ok() {
        let first = burst.clock.now().mono;
        loop {
            let waited = burst.clock.now().mono.saturating_sub(first);
            let left = timing.max_wait.saturating_sub(waited);
            if left.is_zero() {
                break;
            }
            match changes.recv_timeout(timing.quiet.min(left)) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        let Some(look) = look(burst, timing.recheck, changes) else {
            return;
        };
        let changed = FolderChanged {
            matter: burst.matter,
            generation: burst.generation,
            look,
        };
        if post.try_send(Input::PapersChanged(changed)).is_err() {
            return;
        }
    }
}

/// What became of `folder`, whose device and inode were `root`. Only a folder that is not there
/// (`folder_state::stat`: not found, or not a folder), twice `recheck` apart, is missing; a stat
/// that fails otherwise (an offline share, permissions) leaves it found. The wait between the two
/// is on `changes`, whose changes it folds in until `recheck` has passed on the host's clock (or
/// a whole `recheck` of real time went quiet); none once the watch has dropped.
fn look(burst: &Burst, recheck: Duration, changes: &Receiver<()>) -> Option<Look> {
    let (folder, root) = (burst.folder.as_path(), burst.root);
    if stat(folder) == FolderState::Missing {
        let until = burst.clock.now().mono.saturating_add(recheck);
        loop {
            let left = until.saturating_sub(burst.clock.now().mono);
            if left.is_zero() {
                break;
            }
            match changes.recv_timeout(left) {
                Ok(()) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
        if stat(folder) == FolderState::Missing {
            return Some(Look::Missing);
        }
    }
    match (root, root_id(folder)) {
        (Some(was), Some(now)) if was != now => Some(Look::Replaced),
        _ => Some(Look::Found),
    }
}

/// `folder`'s device and inode, when it can be read.
fn root_id(folder: &Path) -> Option<(u64, u64)> {
    std::fs::metadata(folder)
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()))
}

#[cfg(test)]
mod tests;
