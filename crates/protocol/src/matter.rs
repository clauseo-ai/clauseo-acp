//! Matters: the folder a lawyer works in and its checkpoints (the Versions timeline). The
//! matter-level events that carry these types are in `event/payload/matter.rs`.

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};
use crate::{Count, FolderRefId, MatterPath};

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

/// How far the scan of a folder before it becomes a matter has got, after
/// `HostCommand::ScanFolder`: an app-wide event (`HostEvent::FolderScanned`), never logged
/// and never part of a stream. The host sends at most one every 250 ms while the walk runs, and
/// always a last one with `done`. Counts are at most `u32::MAX`; `bytes` is at most
/// [`MAX_SAFE`](crate::MAX_SAFE).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FolderScanned {
    /// The folder, as `ScanFolder` named it.
    pub folder: FolderRefId,
    /// Files an import would version so far: cloud-only and over-cap files are not counted.
    /// When the scan is done and not cancelled, `ImportSummary::files` for the same folder.
    pub files: u32,
    /// Their total size in bytes; when done, `ImportSummary::bytes`.
    pub bytes: Count,
    /// The file the walk reached last, relative to the folder; left out before the first file
    /// and in the last event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now_reading: Option<MatterPath>,
    /// Where the folder lives.
    pub place: FolderPlace,
    /// Cloud-only files so far, counted from their metadata and never read.
    pub cloud_only: u32,
    /// The last event of this scan.
    pub done: bool,
    /// The scan was cancelled (`CancelScan`, or a newer `ScanFolder`); only with `done`.
    pub cancelled: bool,
}

/// Where a folder lives. JSON: `"local"`, `"network"` or `{"synced": {"service": "one_drive"}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FolderPlace {
    /// On a disk of this computer, and no cloud service Clauseo knows syncs it.
    Local,
    /// On a network drive or share.
    Network,
    /// In a folder a cloud service syncs.
    Synced {
        /// The service, from the synced folder's name.
        service: SyncService,
    },
}

/// The cloud service that syncs a folder: [`FolderPlace::Synced`]'s `service`. Closed, as
/// `FolderPlace` is never logged (decision record 0002): a newer value fails to decode. JSON:
/// `"one_drive"`, `"dropbox"`, `"google_drive"`, `"icloud"`, `"box"` or `"other"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum SyncService {
    /// Microsoft `OneDrive`, personal or a work or school account.
    OneDrive,
    /// Dropbox.
    Dropbox,
    /// Google Drive.
    GoogleDrive,
    /// Apple's iCloud Drive.
    #[serde(rename = "icloud")]
    ICloud,
    /// Box.
    Box,
    /// A service Clauseo does not name.
    Other,
}

impl SyncService {
    /// Every value, in declaration order.
    pub const ALL: [Self; 6] = [
        Self::OneDrive,
        Self::Dropbox,
        Self::GoogleDrive,
        Self::ICloud,
        Self::Box,
        Self::Other,
    ];

    /// The service a synced folder's `name` gives: what comes before its first `-`, trimmed
    /// (`OneDrive-Contoso` and `OneDrive - Contoso` are `OneDrive`), compared without case;
    /// `Other` for a name it does not know. What follows the `-` (an account, an email) is
    /// never kept.
    pub fn from_folder_name(name: &str) -> Self {
        let head = name.split('-').next().unwrap_or(name).trim();
        let known = [
            ("onedrive", Self::OneDrive),
            ("dropbox", Self::Dropbox),
            ("googledrive", Self::GoogleDrive),
            ("icloud", Self::ICloud),
            ("icloud drive", Self::ICloud),
            ("box", Self::Box),
        ];
        known
            .into_iter()
            .find(|(spelling, _)| head.eq_ignore_ascii_case(spelling))
            .map_or(Self::Other, |(_, service)| service)
    }
}
