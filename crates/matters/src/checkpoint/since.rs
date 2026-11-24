//! An after-turn checkpoint's changes when other answers in the matter ran at the same time:
//! measured from the turn's own before-turn checkpoint, not the last one, with the papers only
//! another answer changed left out, and those left out listed. See [`Overlap`].

use std::collections::{BTreeMap, BTreeSet};

use clauseo_protocol::{ChangeKind, CheckpointId, ContentId, FileChange, MatterPath, SkippedFile};
use clauseo_protocol::{SkippedFileReason, ThreadId, Timestamp};

use super::{Core, Named, SizedFile, Spec, version_entries};
use crate::diff::{TurnStart, diff_by, flag};
use crate::first_read::FirstReads;
use crate::history::HistoryStore;
use crate::stat::CacheEntry;
use crate::{Checkpoint, CheckpointOutcome, CommitId, Error, rules};

/// Answers in the same matter that ran while this one did: a checkpoint of theirs may lie
/// between this turn's before-turn and after-turn checkpoints, so the turn's changes are
/// measured from its own start, and a paper another answer changed is left out unless this turn
/// wrote it too (`Matter::after_turn_named`).
#[derive(Debug, Clone, Copy)]
pub struct Overlap<'a> {
    /// The turn's own before-turn checkpoint.
    pub since: CheckpointId,
    /// Each paper the other overlapping turns' tool calls or credited shell commands named, with
    /// those turns' threads.
    pub others: &'a BTreeMap<MatterPath, BTreeSet<ThreadId>>,
    /// The papers of `others` that only answers which have ended named, each with the content
    /// the card of the last of them left it with (`None`: deleted). Such a paper is left out
    /// only while it still holds that content: changed since, the lawyer may have edited it, and
    /// it is kept, flagged unless this turn wrote it.
    pub left: &'a BTreeMap<MatterPath, Option<ContentId>>,
    /// Each paper answers that ended with no card named (their after-turn checkpoint failed),
    /// with their threads. A change to one is never left out: their work is on no other card, so
    /// it stays here, flagged unless this turn wrote it, naming them in `also_changed_by`.
    pub failed: &'a BTreeMap<MatterPath, BTreeSet<ThreadId>>,
    /// Every paper this turn's own tool calls named, whatever their status, and its shell
    /// commands wrote. A change to one is never left out, even flagged (a call that did not
    /// complete, or still runs): kept with `also_changed_by` when another answer named it too.
    pub own: &'a BTreeSet<MatterPath>,
}

/// The folder as an after-turn checkpoint's walk found it.
pub(super) struct Now<'a> {
    /// Its tree, sorted by path bytes.
    pub(super) tree: &'a [SizedFile],
    /// The walk's entries, sorted by path bytes.
    pub(super) entries: &'a [CacheEntry],
    /// The paths now over the cap.
    pub(super) over_cap: &'a BTreeSet<&'a str>,
    /// What this walk reads for the first time, against the last commit's skip record.
    pub(super) reads: &'a FirstReads,
}

/// What an after-turn checkpoint with an [`Overlap`] reports instead of its diff from the last
/// checkpoint.
pub(super) struct Report<'a> {
    /// The changes since the turn's own start; `None` when that is the last checkpoint, so they
    /// are the diff from it.
    since: Option<Vec<FileChange>>,
    overlap: Overlap<'a>,
}

impl Report<'_> {
    /// The checkpoint `outcome`, of commit `commit`, with the changes it reports (see
    /// [`Report::changes`]) and the files it did not version.
    pub(super) fn checkpoint(
        report: Option<Self>,
        outcome: CheckpointOutcome,
        commit: CommitId,
        from_last: Vec<FileChange>,
        skipped: Vec<SkippedFile>,
    ) -> Checkpoint {
        let (changes, beside) = Self::changes(report, from_last);
        Checkpoint {
            outcome,
            commit,
            changes,
            skipped,
            beside,
        }
    }

    /// The changes a checkpoint reports, `from_last` being its flagged diff from the last
    /// checkpoint, and the papers left out as other answers' (sorted): those and none, without an
    /// overlap.
    pub(super) fn changes(
        report: Option<Self>,
        from_last: Vec<FileChange>,
    ) -> (Vec<FileChange>, Vec<MatterPath>) {
        match report {
            Some(report) => left_out(report.since.unwrap_or(from_last), &report.overlap),
            None => (from_last, Vec::new()),
        }
    }
}

/// What checkpoint `spec`, of the folder `now`, reports when its turn ran with other answers
/// (`Named::overlap`); `None` without.
pub(super) fn report<'a>(
    core: &Core,
    spec: &Spec<'a>,
    now: &Now<'_>,
) -> Result<Option<Report<'a>>, Error> {
    let Some((named, overlap)) = spec
        .tool_paths
        .and_then(|named| named.overlap.map(|overlap| (named, overlap)))
    else {
        return Ok(None);
    };
    let last = core.timeline.head().map(|(id, _, _)| id);
    let since = if last == Some(overlap.since) {
        None
    } else {
        Some(changes_since(core, overlap.since, named, now, spec.at)?)
    };
    Ok(Some(Report {
        since,
        overlap: *overlap,
    }))
}

/// The changes from checkpoint `since` to the folder `now`, each flagged as the turn `named`
/// would flag it at `since`: against that checkpoint's tree and skip record, with what the
/// checkpoints after it and this walk read for the first time. A file one of them read
/// unchanged (a baseline) is compared from that content, as [`Core::changes_between`] does; one
/// this walk reads unchanged is no change. `at` is the checkpoint's time: the turn's end.
fn changes_since(
    core: &Core,
    since: CheckpointId,
    named: Named<'_>,
    now: &Now<'_>,
    at: Timestamp,
) -> Result<Vec<FileChange>, Error> {
    let mut start_tree = Vec::new();
    for file in version_entries(core.history.read_tree(core.timeline.tree(since)?)?)? {
        let size = core.size_of(file.path.as_str(), file.blob)?;
        start_tree.push(SizedFile {
            path: file.path,
            blob: file.blob,
            size,
        });
    }
    let step = match core.timeline.head() {
        Some((head, _, _)) => core.timeline.step_reads(since, head),
        None => crate::first_read::StepReads::default(),
    };
    let baseline: BTreeSet<&str> = now
        .reads
        .baseline
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    let mut old = start_tree.clone();
    // A file a checkpoint after `since` read unchanged: its content there is what the
    // folder held at the turn's start.
    let pinned = step.baseline.iter().filter(|(path, _)| {
        !rules::is_excluded_path(path.as_str()) && !baseline.contains(path.as_str())
    });
    for (path, tree) in pinned {
        let blob = core.history.file_at(*tree, path.as_str())?;
        let file = match blob {
            Some(blob) => Some(SizedFile {
                path: path.clone(),
                blob,
                size: core.size_of(path.as_str(), blob)?,
            }),
            None => None,
        };
        set(&mut old, path, file);
    }
    let without = |files: &[SizedFile]| -> Vec<SizedFile> {
        files
            .iter()
            .filter(|file| !baseline.contains(file.path.as_str()))
            .cloned()
            .collect()
    };
    // Every file read for the first time since the turn's start was there before.
    let mut there: BTreeMap<MatterPath, SkippedFileReason> = step.there;
    for file in &now.reads.changed {
        there.entry(file.path.clone()).or_insert(file.reason);
    }
    let not_renamed: BTreeSet<&str> = there.keys().map(MatterPath::as_str).collect();
    let changes = diff_by(
        &without(&old),
        &without(now.tree),
        now.over_cap,
        &not_renamed,
        &mut |file| Ok(file.size),
    )?;
    let reads = FirstReads {
        skips: now.reads.skips,
        baseline: now.reads.baseline.clone(),
        changed: there
            .iter()
            .map(|(path, reason)| SkippedFile {
                path: path.clone(),
                reason: *reason,
            })
            .collect(),
    };
    let record = core.skip_record(since);
    let start = TurnStart {
        commands: named.commands,
        record: record.as_ref(),
        tree: &start_tree,
        reads: &reads,
        entries: now.entries,
        at,
    };
    Ok(changes
        .into_iter()
        .map(|change| flag(change, named.paths, &start))
        .collect())
}

/// `changes` (flagged) without the ones another overlapping answer made, and the papers left
/// out (both paths of a rename), sorted. A change to a paper (for a rename: either path) another
/// answer named (`others`, `failed`) is kept with those threads in `also_changed_by` when this
/// turn wrote it. When it did not (it is flagged), it is left out, unless this turn's tool calls
/// named it after all (`own`), an answer with no card named it (`failed`), or a paper only
/// answers that have ended named (`left`) no longer holds what their card left it with: then it
/// is kept, flagged, as well.
pub(super) fn left_out(
    changes: Vec<FileChange>,
    overlap: &Overlap<'_>,
) -> (Vec<FileChange>, Vec<MatterPath>) {
    let mut dropped = BTreeSet::new();
    let kept = changes
        .into_iter()
        .filter_map(|change| {
            let from = match change.change() {
                ChangeKind::Renamed { from } => Some(from),
                ChangeKind::Added
                | ChangeKind::Modified
                | ChangeKind::Deleted
                | ChangeKind::Unknown { .. } => None,
            };
            // Each path with what the folder holds there now: a rename's old path holds nothing.
            let now: Vec<(&MatterPath, Option<&ContentId>)> = [(change.path(), change.after())]
                .into_iter()
                .chain(from.map(|from| (from, None)))
                .collect();
            let threads: BTreeSet<ThreadId> = now
                .iter()
                .flat_map(|(path, _)| [overlap.others.get(*path), overlap.failed.get(*path)])
                .flatten()
                .flatten()
                .copied()
                .collect();
            if threads.is_empty() {
                return Some(change);
            }
            let kept = now
                .iter()
                .any(|(path, _)| overlap.own.contains(*path) || overlap.failed.contains_key(*path));
            let unchanged = now
                .iter()
                .filter(|(path, _)| overlap.others.contains_key(*path))
                .all(|(path, now)| {
                    let left = overlap.left.get(*path);
                    left.is_none_or(|was| was.as_ref() == *now)
                });
            if change.may_include_lawyer_edits() && !kept && unchanged {
                dropped.insert(change.path().clone());
                dropped.extend(from.cloned());
                return None;
            }
            Some(change.with_also_changed_by(threads.into_iter().collect()))
        })
        .collect();
    (kept, dropped.into_iter().collect())
}

/// `files` (sorted by path bytes) with `path` as `file`, or without it for `None`.
fn set(files: &mut Vec<SizedFile>, path: &MatterPath, file: Option<SizedFile>) {
    let at =
        files.binary_search_by(|old| old.path.as_str().as_bytes().cmp(path.as_str().as_bytes()));
    match (at, file) {
        (Ok(index), Some(file)) => {
            if let Some(old) = files.get_mut(index) {
                *old = file;
            }
        }
        (Ok(index), None) => {
            files.remove(index);
        }
        (Err(index), Some(file)) => files.insert(index, file),
        (Err(_), None) => {}
    }
}
