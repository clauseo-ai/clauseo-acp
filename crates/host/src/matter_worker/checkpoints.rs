//! The checkpoints a turn's end or a start-up repair takes for the matter, and the reused-id
//! answers the host records late: `turn_checkpoint` answers an end, `repair` an interrupted
//! turn at start-up.

use std::collections::BTreeSet;

use clauseo_matters::{Checkpoint, CheckpointOutcome, Matter, TimelineEntry, TurnMeta};
use clauseo_protocol::{CheckpointId, CheckpointKind, FileChange, MatterPath};

use super::{Reused, TurnCheckpoint};
use crate::error::Error;

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

/// The checkpoint that ends an interrupted turn at start-up, `before` being its before-turn
/// checkpoint.
///
/// - The timeline holds the turn's own after-turn checkpoint (matched by thread and turn): its
///   end ran and was never logged (it failed twice). That checkpoint is taken, with the changes
///   from `before` to it; the folder is not looked at, since another turn may have changed it
///   after the writer was freed.
/// - Otherwise, when another open turn of the matter started after this one (`last` is false)
///   or R8's last checkpoint is not `before`: while a turn runs it holds the matter's writer,
///   so no other turn starts and no other checkpoint is taken until its end frees it. A later
///   turn or checkpoint means the end ran, and having left no after-turn checkpoint of its own,
///   R8 reused `before` (the turn changed nothing) or the checkpoint failed (then the next
///   writer's before-turn checkpoint logged the folder's changes as outside edits). The step is
///   empty: `before` is taken, never a later checkpoint or the folder, which hold another
///   writer's changes.
/// - Otherwise the crash came before the after-turn checkpoint: one is taken now, as at step 7.
pub(super) fn repair(
    matter: &mut Matter,
    meta: &TurnMeta,
    tool_paths: &BTreeSet<MatterPath>,
    before: Option<CheckpointId>,
    last: bool,
) -> Result<TurnCheckpoint, Error> {
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
    if let Some(entry) = own {
        let changes = match before {
            Some(before) if before != entry.id => matter.changes_between(before, entry.id)?,
            Some(_) | None => Vec::new(),
        };
        return reused(matter, entry.id, Some(entry), changes, true);
    }
    if let Some(before) = before
        && (!last || matter.last_checkpoint() != Some(before))
    {
        let entry = timeline
            .iter()
            .rev()
            .find(|entry| entry.id == before)
            .copied();
        return reused(matter, before, entry, Vec::new(), false);
    }
    let checkpoint = matter.after_turn(meta, tool_paths)?;
    turn_checkpoint(matter, checkpoint, before)
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
        },
        reused: Some(Reused {
            entry,
            changes,
            own,
        }),
    })
}
