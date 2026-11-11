//! The timeline: the checkpoints of a matter's history, rebuilt from the commits' trailers.

use std::collections::HashMap;

use clauseo_protocol::{CheckpointId, CheckpointKind};

use crate::first_read::{FirstReads, StepReads};
use crate::history::{CommitRecord, Oid};
use crate::skip_record::SkipsRef;
use crate::types::TimelineEntry;
use crate::{Error, trailers};

/// The checkpoints, oldest first, and their commits, plus the ref's tip.
#[derive(Debug, Default)]
pub(crate) struct Timeline {
    pub(crate) entries: Vec<TimelineEntry>,
    commits: Vec<(Oid, Oid)>,
    /// Each checkpoint's files read for the first time, from its trailers.
    reads: Vec<FirstReads>,
    by_id: HashMap<CheckpointId, usize>,
    /// The commit the ref names and its tree, checkpoint or not: the parent of the next commit
    /// and the tree the stat cache describes.
    tip: Option<(Oid, Oid)>,
}

impl Timeline {
    /// The timeline of `records` (oldest first). A commit without a valid `Clauseo-Checkpoint`
    /// trailer is left out with a warning (it can still be the tip); of two commits with one id,
    /// the later wins.
    pub(crate) fn from_records(records: &[CommitRecord]) -> Self {
        let mut timeline = Self::default();
        for record in records {
            if let Some(entry) = trailers::parse(&record.message) {
                let reads = trailers::first_reads(&record.message);
                timeline.push(entry, record.id, record.tree, reads);
            } else {
                tracing::warn!(commit = ?record.id, "a commit without a checkpoint id");
            }
        }
        timeline.tip = records.last().map(|record| (record.id, record.tree));
        timeline
    }

    pub(super) fn push(&mut self, entry: TimelineEntry, commit: Oid, tree: Oid, reads: FirstReads) {
        self.by_id.insert(entry.id, self.entries.len());
        self.entries.push(entry);
        self.commits.push((commit, tree));
        self.reads.push(reads);
        self.tip = Some((commit, tree));
    }

    /// The last checkpoint: its id, commit and tree. Not always the tip: a later version may
    /// put a commit without a trailer after it.
    pub(crate) fn head(&self) -> Option<(CheckpointId, Oid, Oid)> {
        let entry = self.entries.last()?;
        let (commit, tree) = self.commits.last()?;
        Some((entry.id, *commit, *tree))
    }

    /// The commit the ref names and its tree.
    pub(crate) const fn tip(&self) -> Option<(Oid, Oid)> {
        self.tip
    }

    /// Checkpoint `id`'s place on the timeline, oldest first.
    pub(crate) fn index(&self, id: CheckpointId) -> Option<usize> {
        self.by_id.get(&id).copied()
    }

    /// Whether checkpoint `id` is on the timeline.
    pub(crate) fn contains(&self, id: CheckpointId) -> bool {
        self.by_id.contains_key(&id)
    }

    /// The checkpoint just before `id`, when `id` is on the timeline as a checkpoint of `kind`
    /// and is not the first.
    pub(crate) fn before(&self, id: CheckpointId, kind: CheckpointKind) -> Option<CheckpointId> {
        let index = *self.by_id.get(&id)?;
        if self.entries.get(index)?.kind != kind {
            return None;
        }
        Some(self.entries.get(index.checked_sub(1)?)?.id)
    }

    /// The commit of checkpoint `id`, if it is on the timeline.
    pub(crate) fn commit(&self, id: CheckpointId) -> Option<Oid> {
        self.by_id
            .get(&id)
            .and_then(|index| self.commits.get(*index))
            .map(|(commit, _)| *commit)
    }

    /// What the checkpoints between `a` and `b`, in either order, read for the first time:
    /// those after the earlier up to and including the later; none when either is unknown.
    pub(crate) fn step_reads(&self, a: CheckpointId, b: CheckpointId) -> StepReads {
        let (Some(&a), Some(&b)) = (self.by_id.get(&a), self.by_id.get(&b)) else {
            return StepReads::default();
        };
        let (lo, hi) = (a.min(b), a.max(b));
        StepReads::of(
            (lo + 1..=hi)
                .filter_map(|index| Some((self.reads.get(index)?, self.commits.get(index)?.1))),
        )
    }

    /// What the tip commit says of its skip record: the last checkpoint's when the tip is that
    /// checkpoint or a later version's commit on its tree; otherwise unrecorded.
    pub(super) fn tip_skips(&self) -> SkipsRef {
        match (self.tip, self.commits.last(), self.reads.last()) {
            (Some(tip), Some(last), Some(reads)) if tip.0 == last.0 || tip.1 == last.1 => {
                reads.skips
            }
            _ => SkipsRef::Unrecorded,
        }
    }

    /// What checkpoint `id`'s commit says of its skip record.
    pub(super) fn skips(&self, id: CheckpointId) -> SkipsRef {
        self.by_id
            .get(&id)
            .and_then(|index| self.reads.get(*index))
            .map_or(SkipsRef::Unrecorded, |reads| reads.skips)
    }

    /// The tree of checkpoint `id`.
    pub(crate) fn tree(&self, id: CheckpointId) -> Result<Oid, Error> {
        self.by_id
            .get(&id)
            .and_then(|index| self.commits.get(*index))
            .map(|(_, tree)| *tree)
            .ok_or(Error::UnknownCheckpoint { id })
    }
}
