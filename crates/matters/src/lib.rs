//! `clauseo-matters`: a matter's folder and its history, kept in a shadow git repository outside
//! the folder, so every step of a turn can be undone.
//!
//! # Owns
//!
//! - The matter history format (decision record 0007): `<matters root>/<matter id>/history.git`,
//!   `<folder>/.clauseo/matter.json`, the single ref `refs/clauseo/timeline`, the `Clauseo-*`
//!   trailers, the exclusion list, the stat cache (`history.git/index`), the loose-object
//!   compression rule and the sync order.
//! - Creating and importing a matter (the scan, refused folders, warnings, cancel), opening one
//!   again, locating a moved folder and repairing a deleted `matter.json`, `before-turn` and
//!   `after-turn` checkpoints with their diff, undoing a step, restoring a version (the whole
//!   matter or chosen files), and comparing and reading versions.
//! - The document rules: excluded names, the per-file size cap, cloud-only placeholders (never
//!   read), racily clean files.
//!
//! # Interface
//!
//! - [`Matter`]: [`Matter::create`], [`Matter::import`] and [`Matter::open`] make one, and
//!   [`Matter::discard`] undoes a create or import the host could not record, and
//!   [`remove_marker_of`] removes its `matter.json` again if a sync client put it back;
//!   [`Matter::locate`] opens one from the folder the lawyer picked after it moved, and
//!   [`Matter::relink`] writes a deleted or damaged `matter.json` again where the history says
//!   the folder is; [`same_or_nested`] compares two folders as the refused-folder check does,
//!   and [`check_place`] runs import's place check (drive root, refused list) alone, and
//!   [`check_protected`] its protected-tree check (matters root, refused trees, on Windows a
//!   path too deep to start programs in, [`is_too_deep`]);
//!   [`Matter::before_turn`] and [`Matter::after_turn`] take checkpoints ([`Checkpoint`], with
//!   its [`CommitId`]; [`Matter::commit_of`] gives any checkpoint's, the root one's included);
//!   [`Matter::edited_since`] and [`Matter::undo`] undo a step ([`UndoRequest`], [`Undone`]),
//!   and [`Matter::still_gone`] says which files it deleted are back;
//!   [`Matter::timeline`] lists the checkpoints, rebuilt from the trailers on open;
//!   [`Matter::restore`] restores a version ([`Restore`], [`RestoreFiles`], [`Restored`]) and
//!   [`Matter::preview_restore`] says what it would do ([`RestorePlan`]); [`Matter::compare`]
//!   ([`CompareSide`]), [`Matter::files_at`] ([`VersionFile`]) and [`Matter::read_file`] read
//!   versions and take `&self`; [`Matter::papers`] lists the folder as it is now ([`PaperList`],
//!   [`Paper`]) by the checkpoint walk's rules.
//! - [`held_open`]: whether Word (or, on Windows, any program) has a file of the folder open
//!   ([`HeldOpen`]), by the rule undo and restore refuse with; [`held_open_at`] takes a path
//!   as an agent names it, resolved into the folder by [`matter_path_in`];
//!   [`held_open_beside`] takes the parent folder's owner files from a caller that listed it,
//!   and [`OwnersBeside`] tells it, with no file-system call, which names to ask about.
//!   The host asks them before letting an agent write a file. An owner file a crashed Word left
//!   is passed over by the stale rule (`locks/stale.rs`) on macOS and Windows.
//! - With the `test-support` feature, [`test_support::pretend_word_running_since_epoch`], for
//!   tests that plant an owner file and need it held through the stale rule, and
//!   [`test_support::bring_back_after_delete`], a sync client putting back a file just deleted.
//! - [`scan`]: what importing a folder would take ([`ScanReport`]), before any write.
//! - [`scan_folder`]: the folder scan before a matter, by the import's walk and rules, with
//!   running counts and the file reached ([`ScanProgress`]) and its totals ([`FolderScan`],
//!   [`FolderTotals`]); [`place`]: where a folder lives ([`Place`]: local, network, synced).
//! - Inputs the host passes, so nothing depends on the machine: [`Settings`] (the matters root,
//!   the refused trees, [`Limits`], [`Retention`], the [`PlaceholderProbe`]), [`NewMatter`] (ids,
//!   `created_at`, [`Marker`]), [`TurnMeta`], the refused-folder list, a progress callback and a
//!   cancel flag.
//! - [`Error`], with [`Error::code`] mapping each variant to a `clauseo_protocol::ErrorCode`.
//!
//! # Invariants
//!
//! - Nothing of the history is in the matter folder but `.clauseo/matter.json`. This crate never
//!   sets an environment variable, never runs `git` or any process, never reads or writes a `.git`
//!   in the folder, and opens repositories isolated from every git config and variable.
//! - The timeline only moves forward: one ref, each checkpoint a commit whose parent is the ref's
//!   tip. No empty commits: an unchanged tree reuses the last checkpoint
//!   ([`CheckpointOutcome::Reused`]).
//! - Objects, then the ref and the stat cache, are durable before a checkpoint returns.
//! - Every object read from history is checked against its id: blobs as they stream, trees and
//!   commits before they are parsed. One that does not match is an [`Error::History`] naming it,
//!   and undo and restore then change no file. Opening reads the timeline's commits and the
//!   tip's tree, so [`Matter::open`] fails with that error when one of them is damaged, as it
//!   does for a torn object; nothing is repaired on open. Only a blob's size
//!   ([`VersionFile::size`], checkpoint sizes) comes from its header without the check.
//! - Excluded, over-cap and cloud-only files are never read, written or deleted. On macOS every
//!   read of a file's content in the folder, and every listing of the checkpoint walk and the
//!   scan, runs with the calling thread's download of dataless files off (`no_download.rs`; the
//!   thread's policy is put back after, never the process's), so a file evicted after its stat
//!   is not downloaded: its read fails with `EDEADLK` and it counts as cloud-only, never read
//!   again. A file the probe finds cloud-only by the metadata of the open file is not read
//!   either. `no_download.rs` lists what runs outside the guard.
//! - Files in the folder change only by a temp file in the same folder and a rename; the one
//!   other change is the hidden attribute of `.clauseo` on Windows. Undo and
//!   restore check every target and lock before writing any, and never write or delete a file
//!   whose name the folder holds only spelled otherwise (another case or normalisation). A
//!   restore never deletes. One exception to names matching byte for byte: a folder on the way
//!   that the folder spells otherwise (`sub/` for the history's `Sub/c.txt`) is written into
//!   as the file system finds it, so the next checkpoint records `sub/c.txt`; no file is
//!   replaced.
//! - Names are kept byte for byte: a path is stored, listed and restored exactly as the file
//!   system gave it, in whatever Unicode form. A match that refuses (owner files in
//!   [`held_open`] and [`held_open_at`], refused folders, [`same_or_nested`]) compares names
//!   composed (NFC) everywhere, with case ignored on macOS and Windows, so a name in either
//!   form is caught. A match that allows ([`matter_path_in`], the folder relink writes into)
//!   compares as the OS's file system does: byte for byte on Linux, ignoring case on Windows,
//!   and case and Unicode form on macOS (decision 0007, Unicode names).
//! - The API is synchronous; writes take `&mut Matter`. The writer lock, the turn queue and the
//!   "turn running" check are the host's. No clock is read and no id is minted here. A walk
//!   with the folder (checkpoint, import, preview, compare) that finds files gone after it
//!   listed them sleeps up to 300 ms in all on the calling thread before taking them as
//!   deleted (`checkpoint/walk_folder.rs`), so callers keep it off any thread that must stay
//!   responsive (the host runs it on its matter thread).
//!
//! # Depends on
//!
//! `protocol` (ids, `FileChange`, `SkippedFile`, error codes), `gix` (features `index`,
//! `revision`, `sha1`), `serde` and `serde_json` (`matter.json`), `tracing`, `dunce`; `nix` on
//! macOS for the sync calls, `statfs` and the stale rule's `F_GETLK` (features `fs`, `mount`);
//! `windows` (`Win32_Storage_FileSystem`) on Windows to hide `.clauseo` and to tell a network
//! drive; `process` on macOS and Windows for the running Word processes (the stale rule).
//!
//! # Start reading
//!
//! `matter.rs` (create, import, open), then `checkpoint.rs` (walk, stat cache, tree, diff),
//! `undo.rs` and `restore.rs` (both write through `writeback.rs`), `compare.rs`,
//! `history.rs` (the `HistoryStore` trait over gix), and `no_download.rs` (the read guard).

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod checkpoint;
mod compare;
mod diff;
mod error;
mod folder_scan;
mod fsync;
mod history;
mod locks;
mod long_path;
mod matter;
mod no_download;
mod papers;
mod place;
mod probe;
mod refuse;
mod restore;
mod rules;
mod scan;
mod stat;
#[cfg(test)]
mod test_dir;
#[cfg(feature = "test-support")]
pub mod test_support;
mod trailers;
mod types;
mod undo;
mod walk;
mod write;
mod writeback;

pub use error::{Error, MissingReason, Refusal};
pub use folder_scan::{FolderScan, FolderTotals, ScanProgress, scan_folder};
pub use locks::{
    HeldOpen, OwnersBeside, held_open, held_open_at, held_open_beside, matter_path_in,
};
pub use matter::{Matter, remove_marker_of};
pub use papers::{Paper, PaperList};
pub use place::{Place, place};
pub use probe::{OsProbe, PlaceholderProbe};
pub use refuse::{WINDOWS_FOLDER_LIMIT, check_place, check_protected, is_too_deep, same_or_nested};
pub use scan::scan;
pub use types::{
    Checkpoint, CheckpointOutcome, CommitId, CompareSide, DEFAULT_MARKER, FILE_CAP_BYTES,
    IMPORT_WARN_BYTES, ImportWarning, Imported, LARGEST_FILES, LargeFile, Limits, Marker,
    NewMatter, Progress, Restore, RestoreFiles, RestorePlan, Restored, Retention, ScanReport,
    Scanned, Settings, TimelineEntry, TurnMeta, UndoRequest, Undone, VersionFile,
};
