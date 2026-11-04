//! The walk of a checkpoint: the folder against the stat cache, each file the cache cannot
//! vouch for read by the caller's `read` (stored for a checkpoint, only hashed for a preview or
//! a compare), and what is kept of the entries under a folder that does not read. The walk and
//! every read run under one [`NoDownload`] guard, taken here: this is where every checkpoint,
//! import, restore, preview and compare with the folder reads the folder.
//!
//! A file the walk listed and then found gone, at its stat or its read, is looked at again
//! before it counts as deleted: Word saves by swapping files with renames, so a document can be
//! missing for a moment (clauseo-ai/clauseo-acp-private#224, R9: "a walk treats a file that
//! vanishes mid-walk as changed and retries"). The walk ends its pass, then waits each of
//! [`VANISHED_WAITS`] in turn and takes the stat of every file still gone after each wait; one
//! that is back is read and versioned by its content, one gone after the last wait is deleted.
//! Each wait is for all the files still gone at once, so a walk waits at most the sum
//! (300 ms) however many vanished, and not at all when none did. A folder gone before it is
//! listed is not looked at again: Word swaps files, not folders.

use std::collections::BTreeSet;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason};

use super::Walked;
use crate::Error;
use crate::history::Oid;
use crate::no_download::{NoDownload, not_downloaded};
use crate::stat::{CacheEntry, EntryState, Stat, StatCache};
use crate::types::{Progress, Settings};
use crate::walk::{self, Found, FoundKind};

/// The waits before each new look at the files that vanished during a walk, about 300 ms in
/// all: Word's swap is two renames back to back (on Windows the document is missing between
/// them for a few milliseconds), so the first, short wait finds nearly every such file back;
/// the longer two cover a busy disk or an antivirus scan holding up the second rename. 300 ms
/// added to a checkpoint that saw a save in flight stays well inside a turn's start or end, and
/// a file deleted for good is deleted then, as before.
pub(super) const VANISHED_WAITS: [Duration; 3] = [
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(150),
];

/// Reads one file as a blob: `(path, len, cap, guard)` to the blob and the bytes read.
pub(super) type ReadFile<'a> =
    dyn FnMut(&Path, u64, u64, &NoDownload<'_>) -> Result<(Oid, u64), Error> + 'a;

/// Walks `folder` against `cache` and reads every file `cache` cannot vouch for with `read`
/// (`(path, len, cap, guard)` to blob and bytes read: storing it, or only hashing it).
/// `Ok(None)` when cancelled. A file found over the cap when read is skipped like one the walk
/// found over it, and a file found not downloaded when read (evicted since the walk's stat) like
/// one the walk found cloud-only; the entries under a subfolder that does not read are kept as
/// they were. Reads nothing again after either. A file found gone at its stat or read is looked
/// at again after each of [`VANISHED_WAITS`] (the module's comment), and is deleted only when
/// still gone after the last.
pub(super) fn walk_folder(
    cache: &StatCache,
    folder: &Path,
    settings: &Settings,
    cancel: Option<&AtomicBool>,
    progress: &mut dyn FnMut(Progress),
    read: &mut ReadFile<'_>,
) -> Result<Option<Walked>, Error> {
    let guard = NoDownload::begin(settings.probe.as_ref());
    let cap = settings.limits.effective_cap();
    let Some(listing) = walk::walk(folder, &guard, cap, cancel, &mut |_| {})? else {
        return Ok(None);
    };
    let mut taker = Taker {
        cache,
        folder,
        cap,
        guard: &guard,
        read,
        progress,
        entries: Vec::with_capacity(listing.files.len()),
        skipped: Vec::new(),
        done: Progress { files: 0, bytes: 0 },
        read_any: false,
    };
    let mut vanished = listing.vanished;
    for file in listing.files {
        if cancelled(cancel) {
            return Ok(None);
        }
        if let Some(path) = taker.take(file)? {
            vanished.push(path);
        }
    }
    if !vanished.is_empty() {
        let before = taker.entries.len();
        if !taker.look_again(vanished, cancel)? {
            return Ok(None);
        }
        if taker.entries.len() > before {
            taker
                .entries
                .sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
        }
    }
    let Taker {
        mut entries,
        skipped,
        read_any,
        ..
    } = taker;
    if !listing.unread.is_empty() {
        keep_unread(cache, &mut entries, &listing.unread);
    }
    Ok(Some(Walked {
        entries,
        skipped,
        read_any,
    }))
}

fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// One walk's files made into cache entries.
struct Taker<'w, 'r> {
    cache: &'w StatCache,
    folder: &'w Path,
    cap: u64,
    guard: &'w NoDownload<'w>,
    read: &'w mut ReadFile<'r>,
    progress: &'w mut dyn FnMut(Progress),
    entries: Vec<CacheEntry>,
    skipped: Vec<SkippedFile>,
    done: Progress,
    read_any: bool,
}

impl Taker<'_, '_> {
    /// Makes `file` an entry, reading it unless the cache vouches for it. `Ok(Some(path))`
    /// when the read found it gone: no entry yet.
    fn take(&mut self, mut file: Found) -> Result<Option<MatterPath>, Error> {
        let old = self.cache.get(file.path.as_str());
        let mut state = None;
        if file.kind == FoundKind::File {
            if let Some(blob) = self.cache.clean_blob(file.path.as_str(), &file.stat) {
                state = Some(EntryState::Read(blob));
            } else {
                let path = self.folder.join(file.path.as_str());
                match (self.read)(&path, file.len, self.cap, self.guard) {
                    Ok((blob, bytes)) => {
                        self.done.bytes = self.done.bytes.saturating_add(bytes);
                        self.read_any = true;
                        state = Some(EntryState::Read(blob));
                    }
                    // Gone since the walk listed it: looked at again later.
                    Err(Error::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                        return Ok(Some(file.path));
                    }
                    // Grown over the cap since the walk listed it: skipped, with its stat now.
                    Err(Error::Io { source, .. })
                        if source.kind() == io::ErrorKind::FileTooLarge =>
                    {
                        file.kind = FoundKind::OverCap;
                        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                            file.stat = Stat::from_metadata(&metadata);
                        }
                    }
                    // Evicted since the walk listed it: cloud-only, with its stat now.
                    Err(Error::Io { source, .. }) if not_downloaded(&source) => {
                        file.kind = FoundKind::CloudOnly;
                        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                            file.stat = Stat::from_metadata(&metadata);
                        }
                    }
                    Err(err) => return Err(err),
                }
            }
        }
        let state = if let Some(state) = state {
            state
        } else {
            let reason = if file.kind == FoundKind::CloudOnly {
                SkippedFileReason::CloudOnly
            } else {
                SkippedFileReason::OverSizeCap
            };
            if worth_listing(old, &file.stat) {
                self.skipped.push(SkippedFile {
                    path: file.path.clone(),
                    reason,
                });
            }
            // A versioned file that is now cloud-only keeps its blob: dehydration does
            // not change content. A file over the cap leaves the tree.
            match old.map(|old| old.state.blob()) {
                Some(Some(blob)) if file.kind == FoundKind::CloudOnly => EntryState::Unread(blob),
                Some(_) | None => EntryState::Skipped,
            }
        };
        self.done.files += 1;
        (self.progress)(self.done);
        self.entries.push(CacheEntry {
            path: file.path,
            stat: file.stat,
            state,
        });
        Ok(None)
    }

    /// Looks again at the files `vanished` (gone at their stat or read): after each of
    /// [`VANISHED_WAITS`], takes the stat of every one still gone, and takes each one back as
    /// the walk would have. One still gone after the last wait, no longer a regular file, or
    /// whose stat fails otherwise is left out: deleted. `Ok(false)` when cancelled.
    fn look_again(
        &mut self,
        mut vanished: Vec<MatterPath>,
        cancel: Option<&AtomicBool>,
    ) -> Result<bool, Error> {
        for wait in VANISHED_WAITS {
            if vanished.is_empty() {
                break;
            }
            if cancelled(cancel) {
                return Ok(false);
            }
            pause(wait);
            for path in std::mem::take(&mut vanished) {
                let absolute = self.folder.join(path.as_str());
                let metadata = match std::fs::symlink_metadata(&absolute) {
                    Ok(metadata) => metadata,
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {
                        vanished.push(path);
                        continue;
                    }
                    Err(err) => {
                        tracing::warn!(path = %absolute.display(), error = %err, "left out a file that vanished and does not stat");
                        continue;
                    }
                };
                let Some(kind) = walk::classify(self.guard.probe(), &absolute, &metadata, self.cap)
                else {
                    continue;
                };
                let found = Found {
                    path,
                    stat: Stat::from_metadata(&metadata),
                    len: metadata.len(),
                    kind,
                };
                if let Some(path) = self.take(found)? {
                    vanished.push(path);
                }
            }
        }
        if !vanished.is_empty() {
            tracing::debug!(
                files = vanished.len(),
                "took files still gone after the waits as deleted"
            );
        }
        Ok(true)
    }
}

/// Adds to `entries` (sorted) the entries of `cache` under each folder of `unread` that the
/// walk did not find, as they were, and sorts them again.
fn keep_unread(cache: &StatCache, entries: &mut Vec<CacheEntry>, unread: &[String]) {
    let found: BTreeSet<String> = entries
        .iter()
        .map(|entry| entry.path.as_str().to_owned())
        .collect();
    for dir in unread {
        let prefix = format!("{dir}/");
        let kept = cache.entries.iter().filter(|entry| {
            entry.path.as_str().starts_with(&prefix) && !found.contains(entry.path.as_str())
        });
        entries.extend(kept.cloned());
    }
    entries.sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
    entries.dedup_by(|a, b| a.path == b.path);
}

/// Whether a skipped file goes in a checkpoint's `skipped` list: it is new, or its size or
/// modification time changed. ctime, device and inode are left out: a sync client touches a
/// placeholder's attributes without changing the file.
pub(super) fn worth_listing(old: Option<&CacheEntry>, stat: &Stat) -> bool {
    old.is_none_or(|old| old.stat.size != stat.size || old.stat.mtime != stat.mtime)
}

/// Waits `wait` on the calling thread (the matter worker's). In unit tests it never sleeps:
/// [`pauses`] counts the wait and runs the action a test armed.
fn pause(wait: Duration) {
    #[cfg(test)]
    pauses::pause(wait);
    #[cfg(not(test))]
    #[expect(
        clippy::disallowed_methods,
        reason = "the walk's one wait, on the matter thread; unit tests count it instead (`pauses`)"
    )]
    std::thread::sleep(wait);
}

/// Unit tests only: the walk's waits, counted instead of slept, per thread.
#[cfg(test)]
pub(super) mod pauses {
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    /// What a test runs at each wait, given the wait's number.
    type Action = Box<dyn FnMut(usize)>;

    thread_local! {
        static WAITED: Cell<Duration> = const { Cell::new(Duration::ZERO) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
        static ACTION: RefCell<Option<Action>> = const { RefCell::new(None) };
    }

    /// Runs `action` at each wait, with the wait's number (0 for the first).
    pub(in crate::checkpoint) fn on_pause(action: impl FnMut(usize) + 'static) {
        ACTION.set(Some(Box::new(action)));
    }

    /// How many waits there were and how long they took in all, then forgets them.
    pub(in crate::checkpoint) fn take() -> (usize, Duration) {
        ACTION.set(None);
        (COUNT.take(), WAITED.take())
    }

    pub(super) fn pause(wait: Duration) {
        let n = COUNT.get();
        COUNT.set(n + 1);
        WAITED.set(WAITED.get() + wait);
        ACTION.with_borrow_mut(|action| {
            if let Some(action) = action.as_mut() {
                action(n);
            }
        });
    }
}
