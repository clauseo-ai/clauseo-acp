//! Reading one folder for the chooser: the [`Machine`] it asks (Windows' own in `windows.rs`, a
//! fake in tests), the time limit every read runs under ([`bounded`]), and what of a read the
//! page sees ([`folders`]).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::place::{Place, name_ok};

/// How long one read of a folder may take before the page hears "no answer".
pub(crate) const LIST_WAIT: Duration = Duration::from_secs(2);

/// The most reads left running past their wait; past it a read answers "no answer" at once.
pub(crate) const OUTSTANDING_MAX: usize = 4;

/// The most folders one listing shows.
pub(crate) const LISTING_MAX: usize = 2000;

/// One entry of a folder, as the machine read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// Its name.
    pub(crate) name: String,
    /// Whether it is a folder.
    pub(crate) dir: bool,
    /// Whether it is hidden or a system entry.
    pub(crate) hidden: bool,
    /// Whether it is a link Windows follows to another place (a name-surrogate reparse point:
    /// junction, symbolic link, mount point). A `OneDrive` placeholder is not.
    pub(crate) link: bool,
}

/// How long the session's one read of the drives may take.
pub(crate) const DRIVES_WAIT: Duration = Duration::from_millis(1500);

/// The drives and network servers a session uses, read once when it opens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Drives {
    /// The fixed and removable drives' letters.
    pub(crate) local: Vec<char>,
    /// The network drives' letters.
    pub(crate) remote: Vec<char>,
    /// The servers this machine already uses: those of the network drives and of the known
    /// folders (Documents, Desktop, ...), as written.
    pub(crate) servers: Vec<String>,
    /// The Desktop folder, unchecked.
    pub(crate) desktop: Option<String>,
}

/// What the chooser asks of the machine. Every call may block on a share that is down, so each
/// runs on a thread of its own under a time limit ([`bounded`]).
pub(crate) trait Machine: Send + Sync + 'static {
    /// The drives, the servers in use and the Desktop.
    fn drives(&self) -> Drives;
    /// The entries of `folder`: an error of kind `PermissionDenied` when Windows refused to read
    /// it, of any other kind when it is not there or not a folder.
    fn read(&self, folder: &str) -> std::io::Result<Vec<Entry>>;
    /// What `path` itself is (its name left empty), or `None` when it is not there.
    fn entry(&self, path: &str) -> Option<Entry>;
}

/// Whether `place` is on the network: a share path, or a network drive of `drives`.
pub(crate) fn on_network(drives: &Drives, place: &Place) -> bool {
    place.is_share() || place.letter().is_some_and(|l| drives.remote.contains(&l))
}

/// Runs `work` on a thread of its own and waits at most `wait` for it. `None` when it ran out
/// of time, when [`OUTSTANDING_MAX`] earlier runs are still going (no thread is started then),
/// or when no thread could start. A run past its wait is left to finish, detached: it holds no
/// lock, only `outstanding`'s count and a sender whose receiver is gone.
pub(crate) fn bounded<T: Send + 'static>(
    outstanding: &Arc<AtomicUsize>,
    wait: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    if outstanding.fetch_add(1, Ordering::AcqRel) >= OUTSTANDING_MAX {
        outstanding.fetch_sub(1, Ordering::AcqRel);
        return None;
    }
    let count = Running(Arc::clone(outstanding));
    let (send, answer) = mpsc::sync_channel(1);
    // Not started: the closure, and `count` with it, drops, which frees the place.
    let spawned = std::thread::Builder::new()
        .name("clauseo-chooser-read".into())
        .spawn(move || {
            let done = work();
            drop(count);
            // The receiver is gone when the wait ran out; the answer is then dropped.
            let _gone = send.send(done);
        });
    if spawned.is_err() {
        return None;
    }
    answer.recv_timeout(wait).ok()
}

/// One of [`bounded`]'s places, freed when dropped.
struct Running(Arc<AtomicUsize>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The folders of `entries` the page may see, sorted as Windows sorts names (ignoring case),
/// at most [`LISTING_MAX`], and whether there were more. Files, hidden and system folders,
/// links, and names the chooser would not take are left out.
pub(crate) fn folders(entries: Vec<Entry>) -> (Vec<String>, bool) {
    let mut names: Vec<String> = entries
        .into_iter()
        .filter(|entry| entry.dir && !entry.hidden && !entry.link && name_ok(&entry.name))
        .map(|entry| entry.name)
        .collect();
    names.sort_by_cached_key(|name| (name.to_lowercase(), name.clone()));
    let more = names.len() > LISTING_MAX;
    names.truncate(LISTING_MAX);
    (names, more)
}
