//! Matters: the folder a lawyer works in, its checkpoints (the Versions timeline), and the
//! matter-level events, which are logged in the matter's stream, never a thread's.

use serde::{Deserialize, Serialize};

use crate::open_enum::open_unit_enum;
use crate::{CheckpointId, FileChange, MatterPath, ThreadId, TurnId};

/// How a matter was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum MatterKind {
    /// The built-in matter the host creates on first start.
    Scratch,
    /// A folder the lawyer chose or created.
    Folder,
}

/// What the first checkpoint of a folder matter found. Counts are at most `u32::MAX`; `bytes`
/// is at most [`MAX_SAFE`](crate::MAX_SAFE).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ImportSummary {
    /// Files versioned.
    pub files: u32,
    /// Their total size in bytes.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub bytes: u64,
    /// Cloud placeholders skipped because their content is not on this computer.
    pub cloud_only: u32,
}

open_unit_enum! {
    /// Why a checkpoint of the matter's files was taken.
    pub enum CheckpointKind {
        /// The first checkpoint of a folder matter.
        Import,
        /// Before a turn ran.
        BeforeTurn,
        /// After a turn ended.
        AfterTurn,
        /// Before an undo or a restore wrote anything, because the folder changed since the
        /// last checkpoint ("You, outside Clauseo").
        OutsideEdits,
        /// An undo of a step.
        Undo,
        /// A restore of an earlier version.
        Restore,
    }
}

/// What a restore puts back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum RestoreScope {
    /// The whole matter.
    Matter,
    /// These files only.
    Files(Vec<MatterPath>),
}

/// One file's choice in an undo, sent in a command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FileDecision {
    /// The file, relative to the matter folder.
    pub path: MatterPath,
    /// Whether to undo it.
    pub choice: FileChoice,
}

/// Undo a file's change, or keep it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FileChoice {
    /// Put the file back as it was before the step.
    Undo,
    /// Leave it as it is.
    Keep,
}

/// The payload of `DomainEvent::MatterCreated`, the first event of a matter's stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MatterCreated {
    /// The matter's name.
    pub name: String,
    /// How it was made.
    pub kind: MatterKind,
    /// What the import found, for a folder matter.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub import: Option<ImportSummary>,
}

/// The payload of `DomainEvent::CheckpointRecorded`: one entry of the Versions timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CheckpointRecorded {
    /// The checkpoint.
    pub checkpoint: CheckpointId,
    /// Why it was taken.
    pub kind: CheckpointKind,
    /// The thread whose turn or undo took it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub thread: Option<ThreadId>,
    /// The turn it was taken around, or the step an undo reverted.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub turn: Option<TurnId>,
}

/// The payload of `DomainEvent::OutsideEdits`: files changed outside Clauseo between turns (the
/// lawyer, Word, a sync client), captured in `checkpoint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct OutsideEdits {
    /// The checkpoint that captured them.
    pub checkpoint: CheckpointId,
    /// The files.
    pub files: Vec<FileChange>,
}

/// The payload of `DomainEvent::VersionRestored`.
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
