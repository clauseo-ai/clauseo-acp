//! Protected names at a matter's root that changed in a session, set aside where the agent
//! can't reach them (S15 PR3; the review's H2 and M6). NTFS holds no entry for a missing name,
//! so a protected name ([`PROTECTED_NAMES`](crate::PROTECTED_NAMES), and `.git`) that doesn't
//! exist at launch gets no deny, and the agent can make it; it can also rename or hard-link a
//! lawyer's own file or folder to one, which keeps the lawyer as the owner. So the check goes by
//! change, not by owner:
//!
//! - At launch (`before_launch`), each protected name at the matter's root is recorded with its
//!   file ID (the volume's serial and the 128-bit id, read from a handle), or as absent, and the
//!   record is marked open.
//! - When the agent exits (`after_session`), every protected name that is new, or whose file ID
//!   changed, is moved aside, whoever owns it; then the record is marked clean.
//! - A session that didn't end cleanly leaves the record open: the next launch runs the same
//!   check against it before anything else, then records afresh.
//! - The strict walk moves one item the lawyer can't open the same way (`set_aside_path`).
//!
//! A protected name the lawyer changes during a session is set aside too, which is accepted.
//!
//! Everything lives in `<data_dir>\sandbox\set-aside\<matter>\`, under the sandbox folder's
//! protected DACL, never in the state folder, which the agent can write: `record.json` (the
//! names at launch), `manifest.json` (each item set aside: [`Entry`]), `set-aside.lock`, and one
//! folder per pass named for its UTC time (`YYYYMMDDTHHMMSS-mmmZ`) holding the items. On Windows
//! each part of that folder is opened relative to its parent's handle, never through a link,
//! and each item is moved by a rename on its own handle (`ntfs.rs`). `list_set_aside` and
//! `restore_set_aside` show and undo a set-aside; a restore never overwrites.
//!
//! Known limits: a nested `CLAUDE.md` the agent makes below the root is not checked here. A
//! matter on another volume than the data folder can't be renamed into it, and nothing is
//! copied: the set-aside fails with [`SetAsideFailure::OtherVolume`] and the launch is refused
//! until the name is moved or removed by hand. An item moved aside keeps its own DACL, so an
//! agent it grants could still open it by its full path; it no longer loads in the matter.

use std::io;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::time::SystemTime;

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

#[cfg(any(windows, test))]
mod files;
#[cfg(windows)]
mod ntfs;
#[cfg(any(windows, test))]
mod run;

/// Why an item was set aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The name wasn't there at launch.
    New,
    /// The name was there at launch as another file or folder: its file ID changed.
    Changed,
    /// The lawyer can't open it: its permissions shut the lawyer out.
    LawyerCantOpen,
}

/// One item set aside, as the manifest lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Its number in the matter's manifest, never reused.
    pub id: u64,
    /// The matter it was in.
    pub matter: MatterId,
    /// Where it was, in the matter.
    pub from: PathBuf,
    /// Where it is now: `<data_dir>\sandbox\set-aside\<matter>\<folder>\<name>`.
    pub to: PathBuf,
    /// The pass's folder, named for its UTC time.
    pub folder: String,
    /// Its name in that folder: its own name, with `~2` and so on added if that was taken.
    pub name: String,
    /// Why it was moved.
    pub reason: Reason,
    /// Its owner's SID when it was moved, for information; none when it couldn't be read.
    pub owner: Option<String>,
    /// When it was moved, in UTC (RFC 3339, to the millisecond).
    pub at: String,
}

/// Why a set-aside, its record or a restore failed. Names no path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SetAsideFailure {
    /// Something is at the name the item would move to; nothing was overwritten.
    #[error("the name is taken")]
    NameTaken,
    /// The item isn't there any more, or the manifest no longer lists it.
    #[error("the item is gone")]
    Gone,
    /// The matter is on another volume than the data folder, and an item can't be renamed
    /// across volumes (`ERROR_NOT_SAME_DEVICE`); nothing is copied.
    #[error("the matter is on another drive than Clauseo's data folder")]
    OtherVolume,
    /// A folder on the way is a link (a reparse point), or the item's final path isn't the one
    /// planned.
    #[error("a folder on the way is a link, or the item moved")]
    Changed,
    /// The record or the manifest is missing where one is needed, or can't be read or trusted.
    #[error("the set-aside record is missing or can't be read")]
    Record,
    /// A file-system call failed, of this kind.
    #[error("a file-system call failed ({0})")]
    Io(io::ErrorKind),
}

/// The launch-time call, first thing before anything else is prepared: when the last
/// session's record is still open (no [`after_session`] ran), sets aside each protected name
/// at the matter's root that is new or changed since that launch; then records each name's
/// file ID afresh and marks the record open. Returns what moved, for the lawyer's notice. On
/// failure the launch is refused and the record stays as it was.
#[cfg(windows)]
pub fn before_launch(
    policy: &crate::Policy,
    data_dir: &Path,
    matter: &MatterId,
    now: SystemTime,
) -> Result<Vec<Entry>, crate::Error> {
    run::before_launch(&ntfs::Ntfs, policy.matter(), data_dir, matter, now)
        .map_err(crate::Error::SetAside)
}

/// The call when the agent has exited: sets aside each protected name at the matter's root
/// that is new or changed since [`before_launch`], whoever owns it, then marks the record
/// clean. Does nothing when the record is clean already. Returns what moved. On failure the
/// record stays open, so the next launch checks again.
#[cfg(windows)]
pub fn after_session(
    policy: &crate::Policy,
    data_dir: &Path,
    matter: &MatterId,
    now: SystemTime,
) -> Result<Vec<Entry>, crate::Error> {
    run::after_session(&ntfs::Ntfs, policy.matter(), data_dir, matter, now)
        .map_err(crate::Error::SetAside)
}

/// Moves the one item at `path` in the matter (the strict walk's child the lawyer can't open:
/// the `DELETE` open succeeds through the parent's `FILE_DELETE_CHILD`) into this pass's
/// set-aside folder, with a manifest entry giving `reason`.
#[cfg(windows)]
pub fn set_aside_path(
    data_dir: &Path,
    matter: &MatterId,
    path: &Path,
    reason: Reason,
    now: SystemTime,
) -> Result<Entry, crate::Error> {
    run::set_aside_path(&ntfs::Ntfs, data_dir, matter, path, reason, now)
        .map_err(crate::Error::SetAside)
}

/// Every item set aside from the matter and not restored, oldest first.
#[cfg(windows)]
pub fn list_set_aside(data_dir: &Path, matter: &MatterId) -> Result<Vec<Entry>, crate::Error> {
    run::list(data_dir, matter).map_err(crate::Error::SetAside)
}

/// Moves `entry`'s item back to where it was, only if that name is free, and drops it from the
/// manifest. Never overwrites: a taken name is [`SetAsideFailure::NameTaken`], an item no
/// longer there (or no longer listed) is [`SetAsideFailure::Gone`].
#[cfg(windows)]
pub fn restore_set_aside(data_dir: &Path, entry: &Entry) -> Result<(), SetAsideFailure> {
    run::restore(&ntfs::Ntfs, data_dir, entry)
}

#[cfg(test)]
mod tests;
// Two attributes, as in `windows.rs`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
