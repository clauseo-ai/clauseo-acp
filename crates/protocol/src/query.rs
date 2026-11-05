//! Queries: reads a client makes without changing anything. They need no receipt, and every
//! field is checked by its type when the query decodes ([`PageLimit`], [`ReadLen`], [`Count`]),
//! so a query has no `validate` step; a refused query fails as `command.invalid`.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::open_enum::open_unit_enum;
use crate::{
    AgentId, AgentKind, BlobHash, CheckpointId, ClientName, CompactString, Count, Date, Error,
    FileChange, FileRefId, LastStop, MatterId, MatterKind, MatterPath, MatterSource, MatterStatus,
    RestoreRequest, SearchAnswer, SearchRequest, Seq, SettingsView, SkippedFile, StreamId,
    ThreadId, ThreadStatus, Timestamp, TurnId, UpdateView, ViewEntry,
};

/// Most entries one `PageEntries` returns: the size of a `Reset`'s view window (about 200 recent
/// entries), so a page is never larger than the `Reset` it continues from.
pub const MAX_PAGE_ENTRIES: u32 = 200;

/// Most bytes one `ReadBlob` returns (1 MiB): one reply stays bounded, and every blob is text
/// today.
pub const MAX_BLOB_READ_BYTES: u32 = 1024 * 1024;

/// Most entries one `ListPapers` returns, papers, skipped files and unread folders together:
/// one reply stays bounded however large the folder.
pub const MAX_PAPERS: u32 = 2_000;

/// Defines a newtype over `u32` in `1..=$max`, which its constructor and its decoder both
/// check, so an out-of-range value never reaches the host.
macro_rules! bounded_u32 {
    ($(#[doc = $doc:expr])* pub struct $name:ident(1..=$max:ident);) => {
        $(#[doc = $doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "specta", derive(specta::Type), specta(type = u32))]
        pub struct $name(u32);

        impl $name {
            #[doc = concat!("Wraps `value`, which must be in 1..=[`", stringify!($max), "`].")]
            pub fn new(value: u32) -> Result<Self, Error> {
                if (1..=$max).contains(&value) {
                    Ok(Self(value))
                } else {
                    Err(Error::OutOfRange {
                        kind: stringify!($name),
                        value: i128::from(value),
                        min: 1,
                        max: i128::from($max),
                    })
                }
            }

            /// The number inside.
            pub const fn get(self) -> u32 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u32(self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
            }
        }
    };
}

bounded_u32! {
    /// How many entries one `PageEntries` asks for: 1 to [`MAX_PAGE_ENTRIES`].
    pub struct PageLimit(1..=MAX_PAGE_ENTRIES);
}
bounded_u32! {
    /// How many bytes one `ReadBlob` asks for: 1 to [`MAX_BLOB_READ_BYTES`].
    pub struct ReadLen(1..=MAX_BLOB_READ_BYTES);
}

/// A read. JSON: `"list_matters"` or `{"list_threads": {...}}`; unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum HostQuery {
    /// Every matter on Clauseo's list (a forgotten matter is not), oldest first.
    ListMatters,
    /// The threads of one matter.
    ListThreads {
        /// The matter.
        matter: MatterId,
    },
    /// Older entries of a stream's view, for scrolling back: up to `limit` entries with an
    /// `order` below `before_order`, newest last.
    PageEntries {
        /// The stream.
        stream: StreamId,
        /// Entries before this order; `None` pages from the newest entry.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        before_order: Option<Seq>,
        /// How many.
        limit: PageLimit,
    },
    /// The files that differ between two versions of a matter, or between a version and the
    /// matter's folder as it is now when `to` is `None` (nothing is stored).
    CompareVersions {
        /// The matter.
        matter: MatterId,
        /// The older version.
        from: CheckpointId,
        /// The newer version; `None` (missing or `null` in JSON) is the folder now.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        to: Option<CheckpointId>,
    },
    /// What restoring a version would do now, file by file. Writes nothing.
    PreviewRestore {
        /// The matter.
        matter: MatterId,
        /// The version to restore.
        checkpoint: CheckpointId,
        /// The whole matter or chosen files, as `RestoreVersion` takes it.
        scope: RestoreRequest,
    },
    /// What undoing a turn's step would meet now: the step's files whose content in the folder
    /// differs from the step's after-turn checkpoint (edited since). Writes nothing and logs
    /// nothing. Refused as `command.invalid` when the turn has no step to undo (not started or
    /// ended in this thread, no after-turn checkpoint, or undone already).
    PreviewUndo {
        /// The thread.
        thread: ThreadId,
        /// The step's turn.
        turn: TurnId,
    },
    /// Part of a blob's text.
    ReadBlob {
        /// The blob.
        hash: BlobHash,
        /// The first byte to read, at most [`MAX_SAFE`](crate::MAX_SAFE).
        offset: Count,
        /// How many bytes.
        len: ReadLen,
    },
    /// The display settings in force, and the keys the firm's policy locks.
    GetSettings,
    /// The files of a matter's folder as it is now, for its file list (`QueryResult::Papers`).
    ListPapers {
        /// The matter.
        matter: MatterId,
    },
    /// The agents in the settings, for a new thread's choice of agent.
    ListAgents,
    /// Where `MatterSource::DefaultLocation` makes new matters, for display: the form's folder
    /// line before a pick. JSON: `"get_default_location"`.
    GetDefaultLocation,
    /// Search inside matters and conversations (decision 0043). Answered
    /// `QueryResult::Search`.
    Search(SearchRequest),
    /// The Windows folder check of the place a `CreateMatter` with `source` would use, before
    /// the matter exists: the New matter dialog's warning. Send the same `source` the create
    /// will. `existing_folder` checks that folder; `new_folder` its picked parent;
    /// `default_location` the default location (its nearest folder that exists, before the
    /// first matter makes it). Answered `QueryResult::FolderSupport` with the values a matter's
    /// row shows (`MatterSummary::folder_support`). What `CreateMatter` refuses before its
    /// matter is made is refused here with the same code (`matter.folder_is_home` and the
    /// rest, for an existing folder), and an unknown ref is `command.invalid`. A check that
    /// doesn't answer in time answers `check_failed`. JSON:
    /// `{"folder_support": {"source": {"existing_folder": {"folder": "..."}}}}`.
    FolderSupport {
        /// Where the matter would be made, as `CreateMatter` names it.
        source: MatterSource,
    },
    /// Where the updater stands (decision record 0037), read after subscribing to the app
    /// stream as `GetSettings` is. Answered `QueryResult::Update`. JSON: `"get_update"`.
    GetUpdate,
}

/// A query's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum QueryResult {
    /// For `ListMatters`.
    Matters(Vec<MatterSummary>),
    /// For `ListThreads`.
    Threads(Vec<ThreadSummary>),
    /// For `PageEntries`, newest last; fewer than `limit` means none older is left.
    Entries(Vec<ViewEntry>),
    /// For `CompareVersions`.
    FileChanges(Vec<FileChange>),
    /// For `PreviewRestore`.
    RestorePreview(RestorePreview),
    /// For `PreviewUndo`.
    UndoPreview(UndoPreview),
    /// For `ReadBlob`.
    Blob(BlobRange),
    /// For `GetSettings`.
    Settings(SettingsView),
    /// For `GetUpdate`.
    Update(UpdateView),
    /// For `ListPapers`: the matter's folder as it is now, by the rules a checkpoint versions
    /// it by (the same excluded names, size cap and cloud-only files).
    Papers {
        /// The files a checkpoint versions, sorted by path bytes.
        papers: Vec<PaperSummary>,
        /// The files a checkpoint does not version (over the size cap, cloud-only), with why,
        /// sorted by path bytes.
        skipped: Vec<SkippedFile>,
        /// The subfolders that could not be read (permission denied, or not downloaded from
        /// the cloud), sorted by path bytes. What they hold is not listed; a checkpoint keeps
        /// it as it was.
        unread: Vec<MatterPath>,
        /// Whether the folder holds more than [`MAX_PAPERS`] entries: `papers`, `skipped` and
        /// `unread` together are then its first [`MAX_PAPERS`] by path (a folder counted
        /// where `<folder>/` sorts).
        truncated: bool,
    },
    /// For `ListAgents`: one per settings agent whose id is a valid [`AgentId`], sorted by id.
    Agents(Vec<AgentSummary>),
    /// For `GetDefaultLocation`. JSON: `{"default_location": {"display": "..."}}`.
    DefaultLocation(DefaultLocation),
    /// For `Search`: a page of hits, or `Cancelled` when a newer search from the same window
    /// came first.
    Search(SearchAnswer),
    /// For `FolderSupport`. JSON: `{"folder_support": {"support": "network"}}`, or
    /// `{"folder_support": {}}` for a fine folder.
    FolderSupport(PickedFolderSupport),
}

/// A picked folder's check, for `FolderSupport`. A struct, so later fields are additive; it
/// accepts fields it does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PickedFolderSupport {
    /// What keeps Codex's sandbox from working fully in the folder, as a matter's row shows it
    /// (`MatterSummary::folder_support`), `check_failed` included. Missing for a fine folder,
    /// and always off Windows.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub support: Option<FolderSupportKind>,
}

/// One file of a matter's folder, as `ListPapers` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PaperSummary {
    /// The file's id for commands, the same for the same path for the host's whole run.
    pub file: FileRefId,
    /// The file, relative to the matter folder.
    pub path: MatterPath,
    /// Its size in bytes.
    pub size: Count,
    /// When it was last modified.
    pub modified: Timestamp,
}

/// One agent in the settings, for `ListAgents`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentSummary {
    /// Its id, as a thread names it.
    pub id: AgentId,
    /// What it is ([`AgentKind::of`]); `None` for an id that names no known agent, such as
    /// `fake-agent`. Left out of the JSON when `None`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub kind: Option<AgentKind>,
}

/// Where new matters go, for `GetDefaultLocation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DefaultLocation {
    /// The folder for display only, as `MatterSummary::folder_display` writes a matter's: below
    /// the home folder the part after it (`Documents/Clauseo`, no `~`), elsewhere the whole
    /// path. It is `matters.default_location`, or `Documents/Clauseo` when that is not set; a
    /// `DefaultLocation` create makes `<this>/<name>`.
    pub display: String,
}

/// What a restore would do now, for `PreviewRestore`. Each list is sorted by path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RestorePreview {
    /// The files the restore would write, as the change from the folder now to the version
    /// (`modified`, or `added` for a file brought back). Empty when the folder already
    /// stands as the version.
    pub changes: Vec<FileChange>,
    /// Files in the folder now that the version does not have: the restore leaves them.
    pub added_since: Vec<MatterPath>,
    /// Files that differ from the version but the restore cannot write (cloud-only, over the
    /// size cap, behind a symlink, and the like): left as they are.
    pub kept: Vec<MatterPath>,
    /// Files of `changes` open in another program now: `RestoreVersion` would fail with
    /// `matter.file_locked`, naming them, and write nothing.
    pub locked: Vec<MatterPath>,
}

/// What undoing a step would meet now, for `PreviewUndo`. A struct, so later fields (such as
/// the files open in another program) are additive; it accepts fields it does not know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UndoPreview {
    /// The step's files whose content now differs from the step's after-turn checkpoint
    /// (changed, deleted, or added at a path the step deleted), sorted by path. Compared by
    /// content: a file the cache vouches for by its stat is not read, any other is hashed. A
    /// file not read (cloud-only, over the size cap) is not listed. Empty when nothing changed
    /// since: Undo then needs no "changed after Clauseo's edit" ask.
    pub edited_since: Vec<MatterPath>,
}

/// One matter in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MatterSummary {
    /// The matter.
    pub matter: MatterId,
    /// Its name.
    pub name: String,
    /// How it was made.
    pub kind: MatterKind,
    /// The time of the newest event in the matter's stream or any of its threads' streams;
    /// missing only when none has one (never, for a matter made by this version).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_activity: Option<Timestamp>,
    /// The client it is for, once set.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub client: Option<ClientName>,
    /// Its next date, once set.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next_date: Option<Date>,
    /// Whether it is pinned to the top of the rail; missing reads as `false`.
    #[serde(default)]
    pub pinned: bool,
    /// When the lawyer last opened it (`OpenMatter`), once they have.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_opened: Option<Timestamp>,
    /// What it is doing over all its threads, computed when asked. This host always writes
    /// it; missing (an older host's answer) reads as `idle`.
    #[serde(default = "MatterStatus::idle")]
    pub status: MatterStatus,
    /// Its folder for display only, never a path to act on: below the home folder, the part
    /// after it (`Documents/Clauseo/State v. Kunal Desai`, no `~`, the system's separators);
    /// elsewhere the whole path. This host always writes it; missing (an older host's
    /// answer) reads as empty, which the UI shows as no folder line.
    #[serde(default)]
    pub folder_display: String,
    /// Whether its folder is where the host last recorded it: in `ListMatters`, as of this list;
    /// in `MattersChanged`, as the last `ListMatters` read it (an in-memory cache of that read,
    /// not state in the log; `found` before any read and after a relocate). Missing reads as
    /// `found`.
    #[serde(default)]
    pub folder_state: FolderState,
    /// What keeps Codex's sandbox from working fully in its folder, on Windows, as the host last
    /// checked it: when the matter was made and at each agent launch (an in-memory reading, not
    /// state in the log); `check_failed` when that check couldn't answer. Missing when the
    /// folder is fine, before any check since the host started, and off Windows.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub folder_support: Option<FolderSupportKind>,
}

open_unit_enum! {
    /// Whether a matter's folder is where the host recorded it. One `stat` per matter when the
    /// host lists matters: it says nothing about the folder's `.clauseo/matter.json`.
    pub enum FolderState {
        /// A folder is there.
        Found,
        /// No folder is there (moved, deleted, or its drive is not mounted): the matter offers
        /// "Locate folder" (`RelocateMatter`).
        Missing,
    }
    test mod folder_state;
}

open_unit_enum! {
    /// Why Codex's sandbox can't work fully in a matter's folder on Windows: the sandbox adds
    /// permission entries to the folder before Codex may write there. A folder with none of
    /// these has no value.
    pub enum FolderSupportKind {
        /// The lawyer can't change the folder's permissions: Codex starts, but asks before every
        /// file edit, and its shell's writes there fail.
        CantChangePermissions,
        /// The folder is on a network drive: Codex can neither read nor write it, so it is not
        /// started there (`matter.folder_network`).
        Network,
        /// The folder's drive keeps no permissions (FAT, exFAT): Codex is not started there
        /// (`matter.folder_no_permissions`).
        NoPermissions,
        /// The check couldn't answer (an io error, or no folder there): Claude runs in forced
        /// ask, and Codex isn't refused (its own sandbox applies).
        CheckFailed,
    }
    test mod folder_support_kind;
}

/// What an older host, which sent no state, means: the folder is taken as found.
impl Default for FolderState {
    fn default() -> Self {
        Self::Found
    }
}

/// One thread in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadSummary {
    /// The thread.
    pub thread: ThreadId,
    /// Its matter.
    pub matter: MatterId,
    /// Its title, once it has one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    #[cfg_attr(feature = "specta", specta(type = Option<String>))]
    pub title: Option<CompactString>,
    /// Its agent.
    pub agent: AgentId,
    /// Where it stands.
    pub status: ThreadStatus,
    /// Whether its queue is held, as [`HeaderEntry::queue_held`](crate::HeaderEntry::queue_held);
    /// missing reads as `false`.
    #[serde(default)]
    pub queue_held: bool,
    /// Whether the lawyer has seen how its last turn ended: true when no turn ended after its
    /// last `MarkSeen` (or none ended at all). Missing (an older host's answer) reads as `true`.
    #[serde(default = "seen_default")]
    pub seen: bool,
    /// When its snooze ends, as [`HeaderEntry::snoozed_until`](crate::HeaderEntry::snoozed_until):
    /// the host clears it when the time comes. Missing reads as `None`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub snoozed_until: Option<Timestamp>,
    /// Whether it is marked done, as [`HeaderEntry::done`](crate::HeaderEntry::done); missing
    /// reads as `false`.
    #[serde(default)]
    pub done: bool,
    /// How and when its last turn ended, once one has. Known for every thread the host has
    /// loaded since it started (at start-up, every thread not archived in a listed matter).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_stop: Option<LastStop>,
    /// The time of the newest event in its stream.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_activity: Option<Timestamp>,
}

/// What a missing `ThreadSummary::seen` reads as: nothing to show.
const fn seen_default() -> bool {
    true
}

/// Part of a blob as UTF-8 text cut to whole characters.
///
/// `offset` is the byte where `text` starts: the requested one, moved forward to the next
/// character start if it fell inside a character. `end` is the byte just after the last returned
/// character, so the next read starts at `end`, and `end == size_bytes` means the blob is done. A
/// `len` too short for the character at `offset` still returns that one character, so `end`
/// always passes `offset` before the end. Both are at most [`MAX_SAFE`](crate::MAX_SAFE).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BlobRange {
    /// The byte where `text` starts.
    pub offset: Count,
    /// The byte just after `text`.
    pub end: Count,
    /// The text.
    pub text: String,
}
