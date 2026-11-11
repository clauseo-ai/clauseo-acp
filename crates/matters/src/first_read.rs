//! Files read for the first time: a file the last checkpoint skipped (cloud-only, or over the
//! cap) that a walk now reads. It was in the folder before, only never versioned, so it is
//! never "added": undo must never delete it (it has no earlier copy to put back either).
//!
//! - Read with the size and modification time it was skipped with, it is unchanged: a
//!   **baseline**. A cloud-only file an agent only read is one (the read downloads it, with the
//!   same content and mtime). Its content becomes the versioned baseline: it is in the tree but
//!   is not a change of the checkpoint, so nothing reports it and nothing undoes it.
//! - Read with another size or mtime, it **changed** since it was skipped: still a change of
//!   the checkpoint (listed as added), but undo keeps it, neither deleted nor overwritten.
//!
//! - A file new to the folder with the size and mtime of a skipped file gone in the same walk is
//!   that file moved or renamed: changed too.
//! - A versioned file that was cloud-only and not read (its content then unknown), read now with
//!   the size and mtime it had then, is a baseline too: its content is what the folder held, a
//!   colleague's edit in the cloud, say, not a change of this checkpoint.
//!
//! What was skipped comes from the last commit's skip record (`skip_record.rs`), not the stat
//! cache, so a cache rebuilt or lost does not lose it. Each checkpoint's commit records its first
//! reads in `Clauseo-Baseline` and `Clauseo-Was-Skipped` trailers (`trailers.rs`, decision
//! 0007), so an undo or a diff long after still knows.

use std::collections::BTreeMap;

use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason};

use crate::history::Oid;
use crate::skip_record::{SkipEntry, SkipRecord, SkipsRef, same_stat};
use crate::stat::{CacheEntry, EntryState, find_by_path};

/// One checkpoint's files read for the first time, each list sorted by path bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FirstReads {
    /// The checkpoint's skip record (`skip_record.rs`), as its commit names it.
    pub(crate) skips: SkipsRef,
    /// Read unchanged: in the tree, not a change.
    pub(crate) baseline: Vec<SkippedFile>,
    /// Read changed: a change undo never deletes or overwrites.
    pub(crate) changed: Vec<SkippedFile>,
}

impl FirstReads {
    /// Whether there are none.
    pub(crate) fn is_empty(&self) -> bool {
        self.baseline.is_empty() && self.changed.is_empty()
    }
}

/// The files `old` (the skip record of the last commit) has as skipped that `new` (the walk's
/// entries, sorted by path bytes) has as read. One read with the size and mtime it was skipped
/// with is a baseline, one read otherwise changed. A file new to the folder with the size and
/// mtime of a skipped file the walk no longer finds is that file moved or renamed: changed, with
/// its reason, so undo keeps it too. `versioned` says whether a path is in the last tree.
pub(crate) fn first_reads(
    old: &SkipRecord,
    new: &[CacheEntry],
    versioned: &dyn Fn(&str) -> bool,
) -> FirstReads {
    let mut reads = FirstReads::default();
    if old.skipped.is_empty() && old.stale.is_empty() {
        return reads;
    }
    let found = |path: &str| find_by_path(new, path, |entry| entry.path.as_str()).is_some();
    let vanished: Vec<&SkipEntry> = old
        .skipped
        .iter()
        .filter(|entry| !found(entry.path.as_str()))
        .collect();
    for entry in new {
        if !matches!(entry.state, EntryState::Read(_)) {
            continue;
        }
        let file = |reason| SkippedFile {
            path: entry.path.clone(),
            reason,
        };
        // Versioned, then cloud-only and not read at the last commit: read now with that stat,
        // its content is what the folder held then (a colleague's edit, say), not a change.
        if let Some(stale) = old.stale_at(entry.path.as_str()) {
            if same_stat(stale, &entry.stat) {
                reads.baseline.push(file(SkippedFileReason::CloudOnly));
            }
            continue;
        }
        match old.skipped_at(entry.path.as_str()) {
            Some(skip) if same_stat(skip, &entry.stat) => reads.baseline.push(file(skip.reason)),
            Some(skip) => reads.changed.push(file(skip.reason)),
            None if versioned(entry.path.as_str()) => {}
            None => {
                if let Some(skip) = vanished.iter().find(|skip| same_stat(skip, &entry.stat)) {
                    reads.changed.push(file(skip.reason));
                }
            }
        }
    }
    reads
}

/// What the checkpoints between two (after the earlier, up to and including the later) read
/// for the first time, as a diff or an undo of that range needs it.
#[derive(Debug, Clone, Default)]
pub(crate) struct StepReads {
    /// Each baseline, by path, with the tree of the earliest checkpoint that has it: its content
    /// there is what the folder held before, so the earlier side of a diff takes it.
    pub(crate) baseline: BTreeMap<MatterPath, Oid>,
    /// Every file any of them read for the first time, with the reason it had been skipped
    /// (the earliest's): it was in the folder before, so undo never deletes or writes it.
    pub(crate) there: BTreeMap<MatterPath, SkippedFileReason>,
}

impl StepReads {
    /// The range's reads from each of its checkpoints' records and trees, oldest first.
    pub(crate) fn of<'a>(records: impl Iterator<Item = (&'a FirstReads, Oid)>) -> Self {
        let mut step = Self::default();
        for (record, tree) in records {
            for file in &record.baseline {
                step.baseline.entry(file.path.clone()).or_insert(tree);
            }
            for file in record.baseline.iter().chain(&record.changed) {
                step.there.entry(file.path.clone()).or_insert(file.reason);
            }
        }
        step
    }
}

#[cfg(test)]
mod tests;
