//! Matters: the folder a lawyer works in and its checkpoints (the Versions timeline). The
//! matter-level events that carry these types are in `event/payload/matter.rs`.

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};
use crate::{Count, MatterPath};

open_unit_enum! {
    /// How a matter was made.
    pub enum MatterKind {
        /// The built-in matter the host creates on first start.
        Scratch,
        /// A folder the lawyer chose or created.
        Folder,
    }
    test mod matter_kind;
}

/// What the first checkpoint of a folder matter found. Counts are at most `u32::MAX`; `bytes`
/// is at most [`MAX_SAFE`](crate::MAX_SAFE).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ImportSummary {
    /// Files versioned.
    pub files: u32,
    /// Their total size in bytes.
    pub bytes: Count,
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
    test mod checkpoint_kind;
}

open_data_enum! {
    /// What a restore puts back.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum RestoreScope {
        /// The whole matter.
        Matter = "matter",
        /// These files only. JSON: `{"files": {"paths": [..]}}`.
        Files = "files" {
            /// The files, relative to the matter folder.
            paths: Vec<MatterPath>,
        },
        _ =>
        /// A scope from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod restore_scope with [
        RestoreScope::Matter,
        RestoreScope::Files {
            paths: vec![MatterPath::new("a.docx").unwrap()],
        },
    ];
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
