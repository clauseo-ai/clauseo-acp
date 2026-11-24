//! The checkpoints a turn's end or a start-up repair takes for the matter, and the reused-id
//! answers the host records late: `turn_checkpoint` answers an end, `repair` an interrupted
//! turn at start-up, and `root_commit` gives a new matter's root checkpoint's commit.

use clauseo_matters::{Checkpoint, CheckpointOutcome, CommitId, Matter, TimelineEntry, TurnMeta};
use clauseo_protocol::{CheckpointId, CheckpointKind, FileChange, ThreadId};

use super::beside::{self, Beside, Logged};
use super::{Reused, TurnCheckpoint, writes_in};
use crate::error::Error;
use crate::sent::TurnPaths;

/// The timeline entry of checkpoint `id`. A reused id is the last checkpoint, so the search
/// from the end stops at once.
pub(super) fn timeline_entry(matter: &Matter, id: CheckpointId) -> Option<TimelineEntry> {
    matter
        .timeline()
        .iter()
        .rev()
        .find(|entry| entry.id == id)
        .copied()
}

/// `checkpoint` with, when R8 reused an id, its timeline entry and the changes from `before`.
pub(super) fn turn_checkpoint(
    matter: &Matter,
    checkpoint: Checkpoint,
    before: Option<CheckpointId>,
) -> Result<TurnCheckpoint, Error> {
    let CheckpointOutcome::Reused(id) = checkpoint.outcome else {
        return Ok(TurnCheckpoint {
            checkpoint,
            reused: None,
        });
    };
    let entry = timeline_entry(matter, id);
    let changes = match before {
        Some(before) if before != id => matter.changes_between(before, id)?,
        Some(_) | None => Vec::new(),
    };
    Ok(TurnCheckpoint {
        checkpoint,
        reused: Some(Reused {
            entry,
            changes,
            own: false,
        }),
    })
}

/// The start-up repair of a turn the log left open (`MatterJob::Repair`).
#[derive(Debug)]
pub(crate) struct RepairJob {
    pub(crate) meta: TurnMeta,
    /// The turn's tool calls' places, read from its log.
    pub(crate) tool_paths: TurnPaths,
    /// Its before-turn checkpoint, when its log names it.
    pub(crate) before: Option<CheckpointId>,
    /// No other open turn of the matter started after this one, and every thread of the matter
    /// was read.
    pub(crate) last: bool,
    /// The other turns the matter left open, with their tool calls' places.
    pub(crate) open: Vec<(ThreadId, TurnPaths)>,
    /// The cards and outside edits the matter's threads' and its own logs hold; those of
    /// checkpoints after `before` are the turn's others (`beside::of_repair`).
    pub(crate) logged: Vec<Logged>,
}

/// The checkpoint that ends an interrupted turn at start-up, `before` being its before-turn
/// checkpoint. Measured from `before`, the papers other turns changed are left off its card
/// (listed in `beside`): those the other open turns' tool calls wrote or name unfinished, those
/// on the cards logged after `before` while they hold what the card left, and those logged as
/// outside edits since (`beside::of_repair`). So no paper is on its card and another's, or on
/// its card and logged as an outside edit, and none is on no card.
///
/// - The timeline holds the turn's own after-turn checkpoint (matched by thread and turn): its
///   end ran and was never logged (it failed twice). That checkpoint is taken, with the changes
///   from `before` to it, each flagged (no tool call is known for them), without the others'
///   papers; the folder is not looked at, since another turn may have changed it since.
/// - Otherwise, when other turns of the matter were left open too, or every thread was read
///   and R8's last checkpoint is not `before` (another turn ran beside this one and ended, or
///   later turns ran after its end failed): a new after-turn checkpoint is taken, its changes
///   measured from `before` without the others' papers.
/// - Otherwise, when a thread of the matter could not be read (`last` is false), alone with a
///   later checkpoint: the step is empty, `before` taken, never a later checkpoint or the
///   folder, which may hold that thread's open turn's changes; the next before-turn checkpoint
///   logs what this turn changed as outside edits.
/// - Otherwise the crash came before the after-turn checkpoint: one is taken now, as at step 7.
pub(super) fn repair(matter: &mut Matter, job: RepairJob) -> Result<TurnCheckpoint, Error> {
    let RepairJob {
        meta,
        mut tool_paths,
        before,
        last,
        open,
        logged,
    } = job;
    tool_paths.resolve_start();
    let commands = writes_in(matter.folder(), &tool_paths.shell_writes());
    let named = super::tool_paths(matter.folder(), &tool_paths);
    let timeline = matter.timeline();
    let own = timeline
        .iter()
        .rev()
        .find(|entry| {
            entry.kind == CheckpointKind::AfterTurn
                && entry.thread == Some(meta.thread)
                && entry.turn == Some(meta.turn)
        })
        .copied();
    let alone = open.is_empty();
    let measured = before.filter(|before| {
        own.is_some() || !open.is_empty() || (last && matter.last_checkpoint() != Some(*before))
    });
    let others = measured.map(|since| {
        let at = |id: CheckpointId| timeline.iter().position(|entry| entry.id == id);
        let start = at(since);
        let mut later: Vec<(usize, &Logged)> = logged
            .iter()
            .filter_map(|record| Some((at(record.checkpoint)?, record)))
            .filter(|(position, _)| start.is_some_and(|start| *position > start))
            .collect();
        later.sort_by_key(|(position, _)| *position);
        let later: Vec<&Logged> = later.into_iter().map(|(_, record)| record).collect();
        beside::of_repair(matter.folder(), since, open, &later, &tool_paths)
    });
    let overlap = others.as_ref().map(Beside::overlap);
    if let Some(entry) = own {
        let changes = match before {
            Some(before) if before != entry.id => matter.changes_between(before, entry.id)?,
            Some(_) | None => Vec::new(),
        };
        let changes: Vec<FileChange> = changes
            .into_iter()
            .map(FileChange::with_lawyer_edits)
            .collect();
        let (changes, left_out) = match &overlap {
            Some(overlap) => clauseo_matters::left_out(changes, overlap),
            None => (changes, Vec::new()),
        };
        let mut done = reused(matter, entry.id, Some(entry), changes, true)?;
        done.checkpoint.beside = clauseo_matters::beside_of(&left_out);
        done.checkpoint.left_out = left_out;
        return Ok(done);
    }
    if let Some(before) = before
        && overlap.is_none()
        && (!last || matter.last_checkpoint() != Some(before))
    {
        let entry = timeline
            .iter()
            .rev()
            .find(|entry| entry.id == before)
            .copied();
        return reused(matter, before, entry, Vec::new(), false);
    }
    let checkpoint = matter.after_turn_named(&meta, &named, &commands, overlap.as_ref())?;
    // Alone, nothing of its own since a later checkpoint it reuses: the empty step it always got.
    let nothing = alone && overlap.is_some() && checkpoint.changes.is_empty();
    if let (Some(before), CheckpointOutcome::Reused(_), true) =
        (before, checkpoint.outcome, nothing)
    {
        let entry = matter
            .timeline()
            .iter()
            .rev()
            .find(|entry| entry.id == before)
            .copied();
        return reused(matter, before, entry, Vec::new(), false);
    }
    // Measured from its own start, a reused id's changes from `before` are not added.
    turn_checkpoint(matter, checkpoint, before.filter(|_| overlap.is_none()))
}

/// A checkpoint the repair takes from the timeline, answered as R8 answers a reused id, with
/// its commit, so a late record of it gets its row.
fn reused(
    matter: &Matter,
    id: CheckpointId,
    entry: Option<TimelineEntry>,
    changes: Vec<FileChange>,
    own: bool,
) -> Result<TurnCheckpoint, Error> {
    let commit = matter
        .commit_of(id)
        .ok_or_else(|| Error::Internal(format!("the timeline has no checkpoint {id}")))?;
    Ok(TurnCheckpoint {
        checkpoint: Checkpoint {
            outcome: CheckpointOutcome::Reused(id),
            commit,
            changes: Vec::new(),
            skipped: Vec::new(),
            beside: Vec::new(),
            left_out: Vec::new(),
        },
        reused: Some(Reused {
            entry,
            changes,
            own,
        }),
    })
}

/// The commit of a new matter's root checkpoint `root`.
pub(super) fn root_commit(matter: &Matter, root: CheckpointId) -> Result<CommitId, Error> {
    matter
        .commit_of(root)
        .ok_or_else(|| Error::Internal(format!("the new matter has no checkpoint {root}")))
}
