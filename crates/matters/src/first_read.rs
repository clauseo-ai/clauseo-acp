//! Files read for the first time: a file the last checkpoint skipped (cloud-only, or over the
//! cap) that a walk now reads. It was in the folder before, only never versioned, so it is
//! never "added": undo must never delete it (it has no earlier copy to put back either).
//!
//! - Read with the size and modification time it was skipped with, it is unchanged: a
//!   **baseline**. A cloud-only file an agent only read is one (the read downloads it, with the
//!   same content and mtime). Its content becomes the versioned baseline: it is in the tree but
//!   is not a change of the checkpoint, so nothing reports it and nothing undoes it.
//! - Read with another size or mtime, it **changed** since it was skipped: still a change of
//!   the checkpoint (listed as added), never the new name of a renamed file.
//!
//! Each checkpoint's commit records both in `Clauseo-Baseline` and `Clauseo-Was-Skipped`
//! trailers (`trailers.rs`, decision 0007), so an undo or a diff long after still knows.

use std::collections::BTreeMap;

use clauseo_protocol::{MatterPath, SkippedFile, SkippedFileReason};

use crate::stat::{CacheEntry, EntryState};

/// One checkpoint's files read for the first time, each list sorted by path bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FirstReads {
    /// Read unchanged: in the tree, not a change.
    pub(crate) baseline: Vec<SkippedFile>,
    /// Read changed: a change, never a rename's new name.
    pub(crate) changed: Vec<SkippedFile>,
}

impl FirstReads {
    /// Whether there are none.
    pub(crate) fn is_empty(&self) -> bool {
        self.baseline.is_empty() && self.changed.is_empty()
    }
}

/// The files `old` (the cache before a walk) has as skipped that `new` (the walk's entries)
/// has as read; both sorted by path bytes. The reason each was skipped is not kept by the
/// cache: a size over `cap` (the walk's cap) says over the cap, any other cloud-only, which is
/// right while the cap stays as it was (the host's does). A file over a cap since raised reads as
/// cloud-only; the reason only picks the lawyer's words, never what undo does.
pub(crate) fn first_reads(old: &[CacheEntry], new: &[CacheEntry], cap: u64) -> FirstReads {
    let mut reads = FirstReads::default();
    let (mut old_iter, mut new_iter) = (old.iter().peekable(), new.iter().peekable());
    while let (Some(o), Some(n)) = (old_iter.peek(), new_iter.peek()) {
        match o.path.as_str().as_bytes().cmp(n.path.as_str().as_bytes()) {
            std::cmp::Ordering::Less => {
                old_iter.next();
            }
            std::cmp::Ordering::Greater => {
                new_iter.next();
            }
            std::cmp::Ordering::Equal => {
                if o.state == EntryState::Skipped && matches!(n.state, EntryState::Read(_)) {
                    let reason = if u64::from(o.stat.size) > cap {
                        SkippedFileReason::OverSizeCap
                    } else {
                        SkippedFileReason::CloudOnly
                    };
                    let file = SkippedFile {
                        path: n.path.clone(),
                        reason,
                    };
                    if o.stat.size == n.stat.size && o.stat.mtime == n.stat.mtime {
                        reads.baseline.push(file);
                    } else {
                        reads.changed.push(file);
                    }
                }
                old_iter.next();
                new_iter.next();
            }
        }
    }
    reads
}

/// What the checkpoints of a step (after one checkpoint, up to and including another) read for
/// the first time, as a diff or an undo of the step needs it.
#[derive(Debug, Clone, Default)]
pub(crate) struct StepReads {
    /// The baselines of the step's last checkpoint: left out of the step's changes. One from an
    /// earlier checkpoint of a longer step may have changed since, so it is only in `there`.
    pub(crate) baseline: Vec<MatterPath>,
    /// Every file any checkpoint of the step read for the first time, with the reason it had
    /// been skipped: it was in the folder before the step, so it is never a rename's new name.
    pub(crate) there: BTreeMap<MatterPath, SkippedFileReason>,
}

impl StepReads {
    /// The step's reads from each of its checkpoints' records, oldest first; the last is the
    /// step's last checkpoint's. The oldest record of a path gives its reason.
    pub(crate) fn of<'a>(records: impl DoubleEndedIterator<Item = &'a FirstReads>) -> Self {
        let mut step = Self::default();
        let mut records = records.rev().peekable();
        if let Some(last) = records.peek() {
            step.baseline = last.baseline.iter().map(|file| file.path.clone()).collect();
            step.baseline
                .sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
        }
        for record in records {
            for file in record.baseline.iter().chain(&record.changed) {
                step.there.insert(file.path.clone(), file.reason);
            }
        }
        step
    }

    /// Whether `path` is a baseline of the step's last checkpoint.
    pub(crate) fn is_baseline(&self, path: &str) -> bool {
        self.baseline
            .binary_search_by(|known| known.as_str().as_bytes().cmp(path.as_bytes()))
            .is_ok()
    }
}

#[cfg(test)]
mod tests;
