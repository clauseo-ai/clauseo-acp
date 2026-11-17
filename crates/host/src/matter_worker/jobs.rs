//! The undo and restore requests that cross to the matter thread, each owning what it names, and
//! a restore's two jobs as the matter thread runs them.

use clauseo_matters::{Checkpoint, CheckpointOutcome, Matter, Restore, Restored, TimelineEntry};
use clauseo_protocol::{CheckpointId, FileDecision, RestoreRequest, ThreadId, Timestamp, TurnId};

use super::MatterReply;
use super::checkpoints::timeline_entry;
use crate::error::Error;

/// An undo request that owns its decisions, so it can cross to the matter thread.
#[derive(Debug, Clone)]
pub(crate) struct UndoJob {
    pub(crate) before: CheckpointId,
    pub(crate) after: CheckpointId,
    pub(crate) decisions: Vec<FileDecision>,
    pub(crate) thread: Option<ThreadId>,
    pub(crate) turn: Option<TurnId>,
    pub(crate) outside_edits: CheckpointId,
    pub(crate) checkpoint: CheckpointId,
    pub(crate) at: Timestamp,
}

/// A restore request that owns its scope, so it can cross to the matter thread.
#[derive(Debug, Clone)]
pub(crate) struct RestoreJob {
    pub(crate) from: CheckpointId,
    pub(crate) scope: RestoreRequest,
    pub(crate) outside_edits: CheckpointId,
    pub(crate) checkpoint: CheckpointId,
    pub(crate) at: Timestamp,
    /// The restore it takes back, whose added files go too.
    pub(crate) undoes: Option<CheckpointId>,
}

impl RestoreJob {
    /// R8's request, borrowing the scope.
    fn request(&self) -> Restore<'_> {
        Restore {
            undoes: self.undoes,
            from: self.from,
            files: (&self.scope).into(),
            outside_edits: self.outside_edits,
            checkpoint: self.checkpoint,
            at: self.at,
        }
    }
}

/// A restore R8 has done, with the timeline entry of the id it reused when it wrote nothing,
/// so the host can record that id late if the log lacks it.
#[derive(Debug)]
pub(crate) struct RestoreDone {
    pub(crate) restored: Restored,
    /// For a `Reused` restore checkpoint: its kind, thread and turn on R8's timeline.
    pub(crate) reused: Option<TimelineEntry>,
}

/// A restore's first job done: the outside-edits checkpoint, or the last checkpoint the folder
/// equals, with that one's timeline entry, so the host can record it if the log lacks it.
#[derive(Debug)]
pub(crate) struct RecordDone {
    pub(crate) checkpoint: Checkpoint,
    /// For a `Reused` checkpoint: its kind, thread and turn on R8's timeline.
    pub(crate) reused: Option<TimelineEntry>,
}

/// The timeline entry of `outcome`'s id when it is `Reused`.
fn reused(matter: &Matter, outcome: CheckpointOutcome) -> Option<TimelineEntry> {
    match outcome {
        CheckpointOutcome::Reused(id) => timeline_entry(matter, id),
        CheckpointOutcome::Created(_) => None,
    }
}

/// `MatterJob::Restore`: `Matter::restore`.
pub(super) fn restore(matter: Result<&mut Matter, Error>, job: &RestoreJob) -> MatterReply {
    MatterReply::Restored(matter.and_then(|matter| {
        let restored = matter.restore(&job.request())?;
        let reused = reused(matter, restored.checkpoint.outcome);
        Ok(Box::new(RestoreDone { restored, reused }))
    }))
}

/// `MatterJob::RestoreRecord`: `Matter::restore_record`.
pub(super) fn record(matter: Result<&mut Matter, Error>, job: &RestoreJob) -> MatterReply {
    MatterReply::RestoreRecorded(matter.and_then(|matter| {
        let checkpoint = matter.restore_record(&job.request())?;
        let reused = reused(matter, checkpoint.outcome);
        Ok(Box::new(RecordDone { checkpoint, reused }))
    }))
}
