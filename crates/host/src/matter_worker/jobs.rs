//! The undo and restore requests that cross to the matter thread, each owning what it names.

use clauseo_protocol::{CheckpointId, FileDecision, RestoreRequest, ThreadId, Timestamp, TurnId};

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
