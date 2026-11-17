//! Files gone with no copy: the files a checkpoint's walk no longer finds that the last commit
//! skipped (cloud-only, or over the size cap), so no version of them exists to bring back.
//!
//! A deletion of a versioned file is a change (`diff.rs`) and undo puts it back. A skipped file
//! is in no tree, so its deletion is no change at all, and without this list it would vanish
//! from the folder with nothing said. What was skipped at the step's start comes from the last
//! commit's skip record (`skip_record.rs`), the same record undo reads.
//!
//! A skipped file the walk no longer finds is not gone when:
//!
//! - it lies under a folder that did not list this time: nothing there is known;
//! - a file new since the last commit (neither skipped nor versioned then) has its size and
//!   modification time: it was moved or renamed, as `first_read.rs` reads it too.
//!
//! A versioned file that was cloud-only and not read (`stale` in the record) is not listed: its
//! deletion is a change of the tree, and undo puts back its earlier version.

use clauseo_protocol::SkippedFile;

use crate::skip_record::{SkipRecord, same_stat};
use crate::stat::{CacheEntry, find_by_path};

/// The files `start` (the last commit's skip record) has as skipped that the walk's `entries`
/// (sorted by path bytes) no longer hold, sorted by path bytes, each with the reason it was
/// skipped. `now` is the walk's own record, for the folders that did not list; `versioned`
/// says whether a path is in the last tree.
pub(crate) fn gone(
    start: &SkipRecord,
    entries: &[CacheEntry],
    now: &SkipRecord,
    versioned: &dyn Fn(&str) -> bool,
) -> Vec<SkippedFile> {
    if start.skipped.is_empty() {
        return Vec::new();
    }
    let found = |path: &str| find_by_path(entries, path, |entry| entry.path.as_str()).is_some();
    // The files new since the last commit: a gone file's stat on one of them is a move.
    let new: Vec<&CacheEntry> = entries
        .iter()
        .filter(|entry| {
            let path = entry.path.as_str();
            start.skipped_at(path).is_none() && start.stale_at(path).is_none() && !versioned(path)
        })
        .collect();
    start
        .skipped
        .iter()
        .filter(|skip| !found(skip.path.as_str()))
        .filter(|skip| !now.under_unread(skip.path.as_str()))
        .filter(|skip| !new.iter().any(|entry| same_stat(skip, &entry.stat)))
        .map(|skip| SkippedFile {
            path: skip.path.clone(),
            reason: skip.reason,
        })
        .collect()
}

/// Every file the walk's record `now` has as skipped, each with why, sorted by path bytes: the
/// files Clauseo holds no copy of at this checkpoint (`Checkpoint::no_copy`). A `stale` file is
/// not one: its earlier version is in the tree.
pub(crate) fn no_copy(now: &SkipRecord) -> Vec<SkippedFile> {
    now.skipped
        .iter()
        .map(|skip| SkippedFile {
            path: skip.path.clone(),
            reason: skip.reason,
        })
        .collect()
}

#[cfg(test)]
mod tests;
