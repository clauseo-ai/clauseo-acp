//! Protected names in a matter that changed in a session, set aside where the agent can't
//! reach them (S15 PR3; the reviews' H2, M6, N1, N6, N10 and N11). NTFS holds no entry for a
//! missing name, so a protected name ([`PROTECTED_NAMES`](crate::PROTECTED_NAMES), and `.git`)
//! that doesn't exist at launch gets no deny, and the agent can make it; it can also rename or
//! hard-link a lawyer's own file or folder to one, which keeps the lawyer as the owner, so git's
//! ownership check passes and its hooks run as the lawyer. So the check goes by change, not by
//! owner. It covers each protected name at the matter's root, and every `.git`, `.claude` and
//! `.mcp.json` at any depth, which the walks find
//! ([`Found`](crate::windows::Found)):
//!
//! - At launch (`before_launch`), each protected name at the root is recorded with its file ID
//!   (the volume's serial and the 128-bit id, read from a handle), or as absent, and so is each
//!   nested name the launch walk found; the session is added to the record's open sessions.
//! - When the agent exits (`after_session`), every protected name that is new, or whose file ID
//!   changed, is moved aside, whoever owns it; then the names are recorded afresh and the
//!   session is taken out. A nested name the record has and the walk no longer finds is dropped.
//! - While any session is open, running or ended without an after-session pass, each launch
//!   runs the same check before recording afresh. Two sessions of one matter may run at once:
//!   the record is clean only when no session is open. A session that crashed stays open for
//!   good, so every later launch and pass keeps checking, which is fail-safe; and a protected
//!   name a running session's agent made may be set aside when another session starts.
//! - The strict walk moves one item the lawyer can't open the same way (`set_aside_path`).
//!
//! The order a launch follows (PR6): `apply` first, then `before_launch` with what its walk
//! found (`AclWriter::found`); when the agent exits, `repair_after_session` first, then
//! `after_session` with what its walk found. Each `Found` path is in the matter root's form
//! (the root joined with each name below it); one outside the root is ignored.
//!
//! A protected name the lawyer changes during a session is set aside too, which is accepted.
//!
//! Everything lives in `<data_dir>\sandbox\set-aside\<matter>\`, under the sandbox folder's
//! protected DACL, never in the state folder, which the agent can write: `record.json` (the
//! names at launch and the open sessions), `manifest.json` (each item set aside: [`Entry`]),
//! `set-aside.lock`, which every read and write of the record or the manifest holds, and one
//! folder per pass named for its UTC time (`YYYYMMDDTHHMMSS-mmmZ`) holding the items, each at
//! its own path below the matter's root (`<pass>\sub\.git`), so nested names never clash; a
//! real clash gets `~2` up to `~9`. Each move is written to the manifest as an intent first
//! and finished after, and each load of the manifest settles an intent a crash left: the item
//! at its new place finishes the entry, the item still at its old place drops it. On Windows
//! each part of that folder is opened relative to its parent's handle, never through a link,
//! and each item is moved by a rename on its own handle (`ntfs.rs`). `list_set_aside` and
//! `restore_set_aside` show and undo a set-aside; a restore never overwrites.
//!
//! A set-aside that can't happen refuses the launch with [`crate::Error::SetAside`], whose
//! `at` names the item for the running page: an item on another volume than the data folder
//! ([`SetAsideFailure::OtherVolume`]: nothing is copied), or one whose folder doesn't give the
//! lawyer `FILE_DELETE_CHILD` (`Io(PermissionDenied)`). The name must then be moved or removed
//! by hand.
//!
//! Known limits: a nested `CLAUDE.md` or `AGENTS.md` the agent makes below the root is not
//! checked here. An item moved aside keeps its own DACL, so an agent it grants could still open
//! it by its full path; it no longer loads in the matter.

use std::io;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::time::SystemTime;

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

#[cfg(windows)]
use crate::windows::Found;

#[cfg(any(windows, test))]
mod check;
#[cfg(any(windows, test))]
mod files;
#[cfg(any(windows, test))]
mod moves;
#[cfg(windows)]
mod ntfs;
#[cfg(any(windows, test))]
mod run;
#[cfg(any(windows, test))]
mod time;

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
    /// Where it is now: `<data_dir>\sandbox\set-aside\<matter>\<folder>\<within...>\<name>`.
    pub to: PathBuf,
    /// The pass's folder, named for its UTC time.
    pub folder: String,
    /// The folders below the pass's folder it is in: its own folders below the matter's root
    /// (`sub` for `sub\.git`). Empty for a name at the root and for the walk's item.
    pub within: Vec<String>,
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

/// The launch-time call, after `apply` and with what its walk found: when any session of
/// the matter is open (running, or ended with no [`after_session`]), sets aside each
/// protected name that is new or changed since the last pass; then records each name's file ID
/// afresh and adds `session` to the open sessions. Returns what moved, for the lawyer's
/// notice. On failure the launch is refused and the record stays as it was.
#[cfg(windows)]
pub fn before_launch(
    policy: &crate::Policy,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    found: &[Found],
    now: SystemTime,
) -> Result<Vec<Entry>, crate::Error> {
    let seen = run::Seen {
        root: policy.matter(),
        found,
    };
    run::before_launch(&ntfs::Ntfs, &seen, data_dir, matter, session, now)
        .map_err(run::Refused::into_error)
}

/// The call when the agent has exited, after `repair_after_session` and with what its walk
/// found: sets aside each protected name that is new or changed since the last pass, whoever
/// owns it, records the names afresh and takes `session` out of the open sessions. Returns
/// what moved. On failure the record stays as it was, so the next launch checks again.
#[cfg(windows)]
pub fn after_session(
    policy: &crate::Policy,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    found: &[Found],
    now: SystemTime,
) -> Result<Vec<Entry>, crate::Error> {
    let seen = run::Seen {
        root: policy.matter(),
        found,
    };
    run::after_session(&ntfs::Ntfs, &seen, data_dir, matter, session, now)
        .map_err(run::Refused::into_error)
}

/// Moves the one item at `path` (the strict walk's child the lawyer can't open: the `DELETE`
/// open succeeds through the parent's `FILE_DELETE_CHILD`) into this pass's set-aside folder,
/// under its own name, with a manifest entry giving `reason`. A failure's `at` is `path`.
#[cfg(windows)]
pub fn set_aside_path(
    data_dir: &Path,
    matter: &MatterId,
    path: &Path,
    reason: Reason,
    now: SystemTime,
) -> Result<Entry, crate::Error> {
    run::set_aside_path(&ntfs::Ntfs, data_dir, matter, path, reason, now)
        .map_err(run::Refused::into_error)
}

/// Every item set aside from the matter and not restored, oldest first.
#[cfg(windows)]
pub fn list_set_aside(data_dir: &Path, matter: &MatterId) -> Result<Vec<Entry>, crate::Error> {
    run::list(&ntfs::Ntfs, data_dir, matter).map_err(run::Refused::into_error)
}

/// Moves `entry`'s item back to where it was, only if that name is free, and drops it from the
/// manifest. Never overwrites: a taken name is [`SetAsideFailure::NameTaken`], an item no
/// longer there (or no longer listed) is [`SetAsideFailure::Gone`].
#[cfg(windows)]
pub fn restore_set_aside(data_dir: &Path, entry: &Entry) -> Result<(), SetAsideFailure> {
    run::restore(&ntfs::Ntfs, data_dir, entry).map_err(|refused| refused.failure)
}

#[cfg(test)]
mod tests;
// Two attributes, as in `windows.rs`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
