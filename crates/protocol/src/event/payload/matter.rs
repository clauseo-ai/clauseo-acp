//! Payloads of the matter-level events, logged in a matter's stream, never a thread's.

use serde::{Deserialize, Serialize};

use crate::{
    CheckpointId, CheckpointKind, Error, FileChange, ImportSummary, MatterKind, RestoreScope,
    ThreadId, TurnId,
};

/// A matter was created: the first event of a matter's stream.
///
/// Built through [`MatterCreated::new`], which refuses an import summary on the scratch matter;
/// decoding accepts any combination, so an older or foreign entry still reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MatterCreated {
    /// The matter's name.
    name: String,
    /// How it was made.
    kind: MatterKind,
    /// What the import found, for a folder matter.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    import: Option<ImportSummary>,
}

impl MatterCreated {
    /// A matter named `name` made as `kind`, with what its import found. Fails with
    /// [`Error::Format`] when a `Scratch` matter carries an `import`: only a folder matter is
    /// imported.
    pub fn new(
        name: impl Into<String>,
        kind: MatterKind,
        import: Option<ImportSummary>,
    ) -> Result<Self, Error> {
        if kind == MatterKind::Scratch && import.is_some() {
            return Err(Error::Format {
                kind: "MatterCreated",
                rule: "the scratch matter has no import summary",
            });
        }
        Ok(Self {
            name: name.into(),
            kind,
            import,
        })
    }

    /// The matter's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How it was made.
    pub const fn kind(&self) -> MatterKind {
        self.kind
    }

    /// What the import found, for a folder matter.
    pub const fn import(&self) -> Option<ImportSummary> {
        self.import
    }
}

/// A checkpoint was recorded: one entry of the Versions timeline.
///
/// Built through [`CheckpointRecorded::new`], which checks `thread` and `turn` against `kind`;
/// decoding accepts any combination, so an older or foreign entry still reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CheckpointRecorded {
    /// The checkpoint.
    checkpoint: CheckpointId,
    /// Why it was taken.
    kind: CheckpointKind,
    /// The thread whose turn or undo took it; `None` for an import or a restore, which no thread
    /// takes.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    thread: Option<ThreadId>,
    /// The turn it was taken around, or the step an undo reverted.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    turn: Option<TurnId>,
}

impl CheckpointRecorded {
    /// A checkpoint taken for `kind`. Fails with [`Error::Format`] on a combination that means
    /// nothing: an `Import` or `Restore` checkpoint that names a thread or a turn; a
    /// `BeforeTurn`, `AfterTurn` or `Undo` checkpoint missing either; any checkpoint that names
    /// a turn without its thread. An `OutsideEdits` checkpoint names the thread and step of the
    /// undo it came before, or nothing before a restore.
    pub fn new(
        checkpoint: CheckpointId,
        kind: CheckpointKind,
        thread: Option<ThreadId>,
        turn: Option<TurnId>,
    ) -> Result<Self, Error> {
        let refused = match kind {
            CheckpointKind::Import | CheckpointKind::Restore => (thread.is_some()
                || turn.is_some())
            .then_some("an import or restore checkpoint names no thread or turn"),
            CheckpointKind::BeforeTurn | CheckpointKind::AfterTurn | CheckpointKind::Undo => {
                (thread.is_none() || turn.is_none())
                    .then_some("a turn's or an undo's checkpoint names its thread and turn")
            }
            CheckpointKind::OutsideEdits | CheckpointKind::Unknown => (thread.is_none()
                && turn.is_some())
            .then_some("a checkpoint that names a turn names its thread"),
        };
        if let Some(rule) = refused {
            return Err(Error::Format {
                kind: "CheckpointRecorded",
                rule,
            });
        }
        Ok(Self {
            checkpoint,
            kind,
            thread,
            turn,
        })
    }

    /// The checkpoint.
    pub const fn checkpoint(&self) -> CheckpointId {
        self.checkpoint
    }

    /// Why it was taken.
    pub const fn kind(&self) -> CheckpointKind {
        self.kind
    }

    /// The thread whose turn or undo took it.
    pub const fn thread(&self) -> Option<ThreadId> {
        self.thread
    }

    /// The turn it was taken around, or the step an undo reverted.
    pub const fn turn(&self) -> Option<TurnId> {
        self.turn
    }
}

/// Files changed outside Clauseo between turns (the lawyer, Word, a sync client), captured in
/// `checkpoint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct OutsideEdits {
    /// The checkpoint that captured them.
    pub checkpoint: CheckpointId,
    /// The files.
    pub files: Vec<FileChange>,
}

/// An earlier version was restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VersionRestored {
    /// The checkpoint restored from.
    pub from: CheckpointId,
    /// The checkpoint the restore wrote.
    pub checkpoint: CheckpointId,
    /// What was put back.
    pub scope: RestoreScope,
}
