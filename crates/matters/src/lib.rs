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
//!   again, `before-turn` and `after-turn` checkpoints with their diff, and undoing a step.
//! - The document rules: excluded names, the per-file size cap, cloud-only placeholders (never
//!   read), racily clean files.
//!
//! # Interface
//!
//! - [`Matter`]: [`Matter::create`], [`Matter::import`] and [`Matter::open`] make one;
//!   [`Matter::before_turn`] and [`Matter::after_turn`] take checkpoints ([`Checkpoint`]);
//!   [`Matter::edited_since`] and [`Matter::undo`] undo a step ([`UndoRequest`], [`Undone`]);
//!   [`Matter::timeline`] lists the checkpoints, rebuilt from the trailers on open.
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
//! - The timeline only moves forward: one ref, each checkpoint a commit whose parent is the last.
//!   No empty commits: an unchanged tree reuses the last checkpoint
//!   ([`CheckpointOutcome::Reused`]).
//! - Objects, then the ref and the stat cache, are durable before a checkpoint returns.
//! - Excluded, over-cap and cloud-only files are never read, written or deleted.
//! - Files in the folder change only by a temp file in the same folder and a rename.
//! - The API is synchronous; writes take `&mut Matter`. The writer lock, the turn queue and the
//!   "turn running" check are the host's. No clock is read and no id is minted here.
//!
//! # Depends on
//!
//! `protocol` (ids, `FileChange`, `SkippedFile`, error codes), `gix` (features `index`,
//! `revision`, `sha1`), `serde` and `serde_json` (`matter.json`), `tracing`, `dunce`; `nix` on
//! macOS for the sync calls.
//!
//! # Start reading
//!
//! `matter.rs` (create, import, open), then `checkpoint.rs` (walk, stat cache, tree, diff),
//! `undo.rs`, and `history.rs` (the `HistoryStore` trait over gix).

#![deny(
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::unreachable,
    clippy::iter_over_hash_type,
    clippy::wildcard_enum_match_arm
)]

mod checkpoint;
mod diff;
mod error;
mod fsync;
mod history;
mod locks;
mod matter;
mod probe;
mod refuse;
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

pub use error::{Error, MissingReason, Refusal};
pub use matter::Matter;
pub use probe::{OsProbe, PlaceholderProbe};
pub use scan::scan;
pub use types::{
    Checkpoint, CheckpointOutcome, DEFAULT_MARKER, FILE_CAP_BYTES, IMPORT_WARN_BYTES,
    ImportWarning, Imported, LARGEST_FILES, LargeFile, Limits, Marker, NewMatter, Progress,
    Retention, ScanReport, Scanned, Settings, TimelineEntry, TurnMeta, UndoRequest, Undone,
};
