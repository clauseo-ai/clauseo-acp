//! The walk of a checkpoint: the folder against the stat cache, each file the cache cannot
//! vouch for read by the caller's `read` (stored for a checkpoint, only hashed for a preview or
//! a compare), and what is kept of the entries under a folder that does not read. A guard on
//! what a read may touch (cloud placeholders) belongs here.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use clauseo_protocol::{SkippedFile, SkippedFileReason};

use super::Walked;
use crate::Error;
use crate::history::Oid;
use crate::stat::{CacheEntry, EntryState, Stat, StatCache};
use crate::types::{Progress, Settings};
use crate::walk::{self, FoundKind};

/// Reads one file as a blob: `(path, len, cap)` to the blob and the bytes read.
pub(super) type ReadFile<'a> = dyn FnMut(&Path, u64, u64) -> Result<(Oid, u64), Error> + 'a;

/// Walks `folder` against `cache` and reads every file `cache` cannot vouch for with `read`
/// (`(path, len, cap)` to blob and bytes read: storing it, or only hashing it). `Ok(None)` when
/// cancelled. A file found over the cap when read is skipped like one the walk found over it;
/// the entries under a subfolder that does not read are kept as they were.
pub(super) fn walk_folder(
    cache: &StatCache,
    folder: &Path,
    settings: &Settings,
    cancel: Option<&AtomicBool>,
    progress: &mut dyn FnMut(Progress),
    read: &mut ReadFile<'_>,
) -> Result<Option<Walked>, Error> {
    let probe = settings.probe.as_ref();
    let cap = settings.limits.effective_cap();
    let Some(listing) = walk::walk(folder, probe, cap, cancel, &mut |_| {})? else {
        return Ok(None);
    };
    let mut entries = Vec::with_capacity(listing.files.len());
    let mut skipped = Vec::new();
    let mut done = Progress { files: 0, bytes: 0 };
    let mut read_any = false;
    for mut file in listing.files {
        if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
            return Ok(None);
        }
        let old = cache.get(file.path.as_str());
        let mut state = None;
        if file.kind == FoundKind::File {
            if let Some(blob) = cache.clean_blob(file.path.as_str(), &file.stat) {
                state = Some(EntryState::Read(blob));
            } else {
                let path = folder.join(file.path.as_str());
                match read(&path, file.len, cap) {
                    Ok((blob, bytes)) => {
                        done.bytes = done.bytes.saturating_add(bytes);
                        read_any = true;
                        state = Some(EntryState::Read(blob));
                    }
                    // Gone since the walk listed it: it is deleted.
                    Err(Error::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotFound =>
                    {
                        continue;
                    }
                    // Grown over the cap since the walk listed it: skipped, with its stat now.
                    Err(Error::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::FileTooLarge =>
                    {
                        file.kind = FoundKind::OverCap;
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
                skipped.push(SkippedFile {
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
        done.files += 1;
        progress(done);
        entries.push(CacheEntry {
            path: file.path,
            stat: file.stat,
            state,
        });
    }
    if !listing.unread.is_empty() {
        keep_unread(cache, &mut entries, &listing.unread);
    }
    Ok(Some(Walked {
        entries,
        skipped,
        read_any,
    }))
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
