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
//! - [`Matter`]: [`Matter::create`], [`Matter::import`] and [`Matter::open`] make one;
//!   [`Matter::locate`] opens one from the folder the lawyer picked after it moved, and
//!   [`Matter::relink`] writes a deleted or damaged `matter.json` again where the history says
//!   the folder is; [`same_or_nested`] compares two folders as the refused-folder check does;
//!   [`Matter::before_turn`] and [`Matter::after_turn`] take checkpoints ([`Checkpoint`]);
//!   [`Matter::edited_since`] and [`Matter::undo`] undo a step ([`UndoRequest`], [`Undone`]);
//!   [`Matter::timeline`] lists the checkpoints, rebuilt from the trailers on open;
//!   [`Matter::restore`] restores a version ([`Restore`], [`RestoreFiles`], [`Restored`]) and
//!   [`Matter::preview_restore`] says what it would do ([`RestorePlan`]); [`Matter::compare`]
//!   ([`CompareSide`]), [`Matter::files_at`] ([`VersionFile`]) and [`Matter::read_file`] read
//!   versions and take `&self`.
//! - [`scan`]: what importing a folder would take ([`ScanReport`]), before any write.
//! - Inputs the host passes, so nothing depends on the machine: [`Settings`] (the matters root,
//!   [`Limits`], [`Retention`], the [`PlaceholderProbe`]), [`NewMatter`] (ids, `created_at`,
//!   [`Marker`]), [`TurnMeta`], the refused-folder list, a progress callback and a cancel flag.
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
//! - Excluded, over-cap and cloud-only files are never read, written or deleted.
//! - Files in the folder change only by a temp file in the same folder and a rename; the one
//!   other change is the hidden attribute of `.clauseo` on Windows. Undo and
//!   restore check every target and lock before writing any, and never write or delete a file
//!   whose name the folder holds only spelled otherwise (another case or normalisation). A
//!   restore never deletes. One exception to names matching byte for byte: a folder on the way
//!   that the folder spells otherwise (`sub/` for the history's `Sub/c.txt`) is written into
//!   as the file system finds it, so the next checkpoint records `sub/c.txt`; no file is
//!   replaced.
//! - The API is synchronous; writes take `&mut Matter`. The writer lock, the turn queue and the
//!   "turn running" check are the host's. No clock is read and no id is minted here.
//!
//! # Depends on
//!
//! `protocol` (ids, `FileChange`, `SkippedFile`, error codes), `gix` (features `index`,
//! `revision`, `sha1`), `serde` and `serde_json` (`matter.json`), `tracing`, `dunce`; `nix` on
//! macOS for the sync calls; `windows` (`Win32_Storage_FileSystem`) on Windows to hide
//! `.clauseo`.
//!
//! # Start reading
//!
//! `matter.rs` (create, import, open), then `checkpoint.rs` (walk, stat cache, tree, diff),
//! `undo.rs` and `restore.rs` (both write through `writeback.rs`), `compare.rs`, and
//! `history.rs` (the `HistoryStore` trait over gix).

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
mod fsync;
mod history;
mod locks;
mod matter;
mod probe;
mod refuse;
mod restore;
mod rules;
mod scan;
mod stat;
#[cfg(test)]
mod test_dir;
mod trailers;
mod types;
mod undo;
mod walk;
mod write;
mod writeback;

pub use error::{Error, MissingReason, Refusal};
pub use matter::Matter;
pub use probe::{OsProbe, PlaceholderProbe};
pub use refuse::same_or_nested;
pub use scan::scan;
pub use types::{
    Checkpoint, CheckpointOutcome, CompareSide, DEFAULT_MARKER, FILE_CAP_BYTES, IMPORT_WARN_BYTES,
    ImportWarning, Imported, LARGEST_FILES, LargeFile, Limits, Marker, NewMatter, Progress,
    Restore, RestoreFiles, RestorePlan, Restored, Retention, ScanReport, Scanned, Settings,
    TimelineEntry, TurnMeta, UndoRequest, Undone, VersionFile,
};
