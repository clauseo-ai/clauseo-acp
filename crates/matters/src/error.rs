//! The crate's one error type and its mapping to protocol error codes.

use std::fmt;
use std::io;
use std::path::PathBuf;

use clauseo_protocol::{CheckpointId, ErrorCode, MatterId, MatterPath};

/// Every failure of a matter operation. [`Error::code`] gives the code a client sees.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The folder is gone, or its `.clauseo/matter.json` is missing, unreadable or names another
    /// matter (`matter.folder_missing`): the "Locate folder" answer.
    #[error("the matter folder {} is missing: {reason}", folder.display())]
    FolderMissing {
        /// The folder the host passed.
        folder: PathBuf,
        /// Which of the cases it is.
        reason: MissingReason,
    },
    /// The folder may not be a matter (`matter.folder_refused`).
    #[error("{} cannot be a matter: {reason}", folder.display())]
    FolderRefused {
        /// The folder the host passed.
        folder: PathBuf,
        /// Why.
        reason: Refusal,
    },
    /// Files the operation would write are open in another program (`matter.file_locked`).
    /// Nothing was written.
    #[error("files are open in another program: {}", join(paths))]
    FileLocked {
        /// The locked files, sorted.
        paths: Vec<MatterPath>,
    },
    /// The matter has no checkpoint with this id (`command.invalid`).
    #[error("the matter has no checkpoint {id}")]
    UnknownCheckpoint {
        /// The id asked for.
        id: CheckpointId,
    },
    /// A checkpoint would reuse an id already on the timeline (`command.invalid`).
    #[error("checkpoint {id} is already on the timeline")]
    DuplicateCheckpoint {
        /// The id passed.
        id: CheckpointId,
    },
    /// An undo names no decision for a path the step changed (`command.invalid`).
    #[error("the undo has no decision for {path}")]
    MissingDecision {
        /// The path the step changed.
        path: MatterPath,
    },
    /// An undo names a path the step did not change, or names one twice (`command.invalid`).
    #[error("the undo's decision for {path} names no change of the step, or repeats one")]
    UnexpectedDecision {
        /// The path named.
        path: MatterPath,
    },
    /// [`Matter::read_file`](crate::Matter::read_file) names a file the version does not have,
    /// or a restore names one that neither the version nor the folder has (`command.invalid`).
    #[error("{path} is not a file of the version")]
    UnknownFile {
        /// The path named.
        path: MatterPath,
    },
    /// A restore of chosen files names this file more than once (`command.invalid`).
    #[error("the restore names {path} more than once")]
    DuplicateFile {
        /// The path named twice.
        path: MatterPath,
    },
    /// A restore of chosen files names none (`command.invalid`).
    #[error("the restore names no file")]
    NoFilesChosen,
    /// [`Matter::create`](crate::Matter::create) was given a folder that already exists
    /// (`command.invalid`); an existing folder is imported instead.
    #[error("{} already exists; import it instead", folder.display())]
    FolderExists {
        /// The folder.
        folder: PathBuf,
    },
    /// The matter's history folder already exists (`command.invalid`): the id is in use.
    #[error("the history folder {} already exists", git_dir.display())]
    HistoryExists {
        /// The matter's folder under the matters root.
        git_dir: PathBuf,
    },
    /// [`Marker::EmptyRepository`](crate::Marker::EmptyRepository) is not built yet
    /// (`command.invalid`).
    #[error("the empty-repository marker is not built yet")]
    MarkerNotBuilt,
    /// A file operation failed (`matter.checkpoint_failed`).
    #[error("{op} {}: {source}", path.display())]
    Io {
        /// What was being done: "read", "write", "rename", ...
        op: &'static str,
        /// The path it was done to.
        path: PathBuf,
        /// The OS error.
        #[source]
        source: io::Error,
    },
    /// The shadow repository failed, or holds something this version cannot read
    /// (`matter.checkpoint_failed`).
    #[error("history: {op}: {source}")]
    History {
        /// What was being done.
        op: &'static str,
        /// The underlying failure.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Why [`Error::FolderMissing`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingReason {
    /// The folder does not exist (moved, deleted, or its drive is not mounted).
    NoFolder,
    /// The folder has no `.clauseo/matter.json`.
    NoMatterJson,
    /// `matter.json` does not read as this version's format.
    Unreadable,
    /// `matter.json` names another matter.
    OtherMatter(MatterId),
}

impl fmt::Display for MissingReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoFolder => f.write_str("it does not exist"),
            Self::NoMatterJson => f.write_str("it has no .clauseo/matter.json"),
            Self::Unreadable => f.write_str("its .clauseo/matter.json does not read"),
            Self::OtherMatter(id) => write!(f, "its .clauseo/matter.json names matter {id}"),
        }
    }
}

/// Why [`Error::FolderRefused`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A drive root: `/`, `/Volumes/<name>`, a Linux mount point, or `X:\`.
    DriveRoot,
    /// It is an entry of the refused list (the home folder, a system folder), or an ancestor of
    /// one. The entry, as the host passed it.
    Listed(PathBuf),
    /// It holds a `.clauseo/matter.json` naming this matter: it already belongs to one.
    AlreadyMatter(MatterId),
    /// It holds a `.clauseo/matter.json` that does not read, so it may belong to a matter.
    UnreadableMatterJson,
    /// It is, contains or sits inside the matters root.
    MattersRoot,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DriveRoot => f.write_str("it is a drive root"),
            Self::Listed(entry) => write!(f, "it is or contains {}", entry.display()),
            Self::AlreadyMatter(id) => write!(f, "it is already matter {id}"),
            Self::UnreadableMatterJson => {
                f.write_str("its .clauseo/matter.json does not read, so it may be a matter")
            }
            Self::MattersRoot => {
                f.write_str("it is, holds or sits inside Clauseo's matters folder")
            }
        }
    }
}

fn join(paths: &[MatterPath]) -> String {
    let names: Vec<&str> = paths.iter().map(MatterPath::as_str).collect();
    names.join(", ")
}

impl Error {
    /// The protocol error code a client sees for this failure.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::FolderMissing { .. } => ErrorCode::MatterFolderMissing,
            Self::FolderRefused { .. } => ErrorCode::MatterFolderRefused,
            Self::FileLocked { .. } => ErrorCode::MatterFileLocked,
            Self::UnknownCheckpoint { .. }
            | Self::DuplicateCheckpoint { .. }
            | Self::MissingDecision { .. }
            | Self::UnexpectedDecision { .. }
            | Self::UnknownFile { .. }
            | Self::DuplicateFile { .. }
            | Self::NoFilesChosen
            | Self::FolderExists { .. }
            | Self::HistoryExists { .. }
            | Self::MarkerNotBuilt => ErrorCode::CommandInvalid,
            Self::Io { .. } | Self::History { .. } => ErrorCode::MatterCheckpointFailed,
        }
    }

    /// Whether this is an [`Error::Io`] saying a file or folder is not downloaded
    /// ([`crate::no_download::not_downloaded`]).
    pub(crate) fn is_not_downloaded(&self) -> bool {
        matches!(self, Self::Io { source, .. } if crate::no_download::not_downloaded(source))
    }

    /// An [`Error::Io`].
    pub(crate) fn io(op: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            op,
            path: path.into(),
            source,
        }
    }

    /// An [`Error::History`].
    pub(crate) fn history(
        op: &'static str,
        source: impl Into<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::History {
            op,
            source: source.into(),
        }
    }
}
