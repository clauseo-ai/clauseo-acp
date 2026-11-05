//! Protected names and links in a matter that changed in a session, set aside where the agent
//! can't reach them (S15 PR3; the reviews' H2, M6, N1, N6, N10, N11 and round 3's R1 to R4).
//! NTFS holds no entry for a missing name, so a protected name
//! ([`PROTECTED_NAMES`](crate::PROTECTED_NAMES), and `.git`) that doesn't exist at launch gets
//! no deny, and the agent can make it; it can also rename or hard-link a lawyer's own file or
//! folder to one, which keeps the lawyer as the owner, so git's ownership check passes and its
//! hooks run as the lawyer. So the check goes by change, not by owner. It covers each protected
//! name at the matter's root, every `.git`, `.claude` and `.mcp.json` at any depth, and every
//! link (junction or symbolic link) anywhere in the matter tree, whatever its name: a link the
//! agent makes could lead git to a `.git` outside the matter (R2).
//!
//! Each pass finds them with its own scan of the matter tree (`scan.rs`): read-only, with no
//! cap, never following a link, each protected name probed by opening it relative to its
//! folder's handle, so whatever NTFS opens for `.git` counts. It doesn't use the ACL walk's
//! results, so a walk that refuses never skips the check (R1).
//!
//! - At launch (`before_launch`), each protected name at the root is recorded with its file ID
//!   (the volume's serial and the 128-bit id, read from a handle), or as absent, and so is each
//!   nested name and each link the scan found; the session is added to the record's open
//!   sessions.
//! - When the agent exits (`after_session`), every protected name or link that is new, or whose
//!   file ID changed, is moved aside, whoever owns it (a link as itself, never what it names);
//!   then everything is recorded afresh and the session is taken out. A name or link the record
//!   has and the scan no longer finds is dropped.
//! - While any session is open, running or ended without an after-session pass, each launch
//!   runs the same check before recording afresh. Two sessions of one matter may run at once:
//!   the record is clean only when no session is open, and a protected name a running
//!   session's agent made may be set aside when another session starts. A launch is told which
//!   of the matter's sessions still run; every other open session crashed. Its changes are
//!   checked at that launch, then its id is dropped (R3). PR5 and PR6 call `before_launch`
//!   only once a crashed session's job object is closed (kill-on-close), so its agent is dead.
//! - A scan that can't open or list a folder still checks and sets aside what it found, and
//!   the pass names the folder ([`Checked::unscanned`]) for the running page. After the
//!   session, it keeps the session open, so the next launch checks again.
//! - The strict walk moves one item the lawyer can't open the same way (`set_aside_path`).
//!
//! The order a launch follows (PR6): `before_launch`, then `apply`; when the agent exits,
//! `repair_after_session`, then `after_session`. The set-aside and the ACL calls are
//! independent: neither takes the other's result, and a refused `apply` or repair doesn't stop
//! the set-aside's check. `before_launch` comes first, so a crashed session is checked even
//! when `apply` then refuses.
//!
//! A protected name the lawyer changes during a session is set aside too, which is accepted.
//!
//! Everything lives in `<data_dir>\sandbox\set-aside\<matter>\`, under the sandbox folder's
//! protected DACL, never in the state folder, which the agent can write: `record.json` (the
//! names and links at the last pass, and the open sessions), `manifest.json` (each item set
//! aside: [`Entry`]), `set-aside.lock`, which every read and write of the record or the
//! manifest holds, and one folder per pass named for its UTC time (`YYYYMMDDTHHMMSS-mmmZ`)
//! holding the items, each at its own path below the matter's root (`<pass>\sub\.git`), so
//! nested names never clash; a real clash gets `~2` up to `~9`. Each move is written to the
//! manifest as an intent first and finished after, and each load of the manifest settles an
//! intent a crash left: the item at its new place finishes the entry, the item still at its old
//! place drops it. On Windows each part of that folder is opened relative to its parent's
//! handle, never through a link, and each item is moved by a rename on its own handle
//! (`ntfs.rs`). `list_set_aside` and `restore_set_aside` show and undo a set-aside; a restore
//! never overwrites.
//!
//! A set-aside that can't happen refuses the launch with [`crate::Error::SetAside`], whose
//! `at` names the item for the running page: an item on another volume than the data folder
//! ([`SetAsideFailure::OtherVolume`]: nothing is copied), or one whose folder doesn't give the
//! lawyer `FILE_DELETE_CHILD` (`Io(PermissionDenied)`). The name must then be moved or removed
//! by hand.
//!
//! Known limits: a nested `CLAUDE.md` or `AGENTS.md` the agent makes below the root is not
//! checked here. A lawyer's own link that was there at launch and is unchanged stays; a link
//! made or swapped while a session ran is set aside even when the lawyer made it. An item moved
//! aside keeps its own DACL, so an agent it grants could still open it by its full path; it no
//! longer loads in the matter.

use std::ffi::OsString;
use std::io;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::time::SystemTime;

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

#[cfg(any(windows, test))]
mod check;
#[cfg(any(windows, test))]
mod files;
#[cfg(any(windows, test))]
mod moves;
mod names;
#[cfg(windows)]
mod ntfs;
#[cfg(any(windows, test))]
mod run;
#[cfg(any(windows, test))]
mod scan;
#[cfg(any(windows, test))]
mod time;

/// Why an item was set aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The name or link wasn't there at the last pass.
    New,
    /// The name or link was there at the last pass as another file, folder or link: its file
    /// ID changed.
    Changed,
    /// The lawyer can't open it: its permissions shut the lawyer out.
    LawyerCantOpen,
}

/// One item set aside, as the manifest lists it. Its paths and names are kept as the file
/// system gave them, text or not (a lone surrogate, say): the manifest writes one that isn't
/// text as its UTF-16 code units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Its number in the matter's manifest, never reused.
    pub id: u64,
    /// The matter it was in.
    pub matter: MatterId,
    /// Where it was, in the matter.
    #[serde(with = "names::path")]
    pub from: PathBuf,
    /// Where it is now: `<data_dir>\sandbox\set-aside\<matter>\<folder>\<within...>\<name>`.
    #[serde(with = "names::path")]
    pub to: PathBuf,
    /// The pass's folder, named for its UTC time.
    pub folder: String,
    /// The folders below the pass's folder it is in: its own folders below the matter's root
    /// (`sub` for `sub\.git`). Empty for a name at the root and for the walk's item.
    #[serde(with = "names::many")]
    pub within: Vec<OsString>,
    /// Its name in that folder: its own name, with `~2` and so on added if that was taken.
    #[serde(with = "names::one")]
    pub name: OsString,
    /// Why it was moved.
    pub reason: Reason,
    /// Its owner's SID when it was moved, for information; none when it couldn't be read.
    pub owner: Option<String>,
    /// When it was moved, in UTC (RFC 3339, to the millisecond).
    pub at: String,
}

/// What a pass did: the items it set aside, and the folders its scan couldn't open or list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    /// What moved, for the lawyer's notice.
    pub moved: Vec<Entry>,
    /// The folders in the matter the scan couldn't open or list (the matter's own folder, when
    /// it couldn't open that). Nothing inside them was checked: the running page tells the
    /// lawyer, naming them. Empty when the scan was complete.
    pub unscanned: Vec<PathBuf>,
}

impl Checked {
    /// Whether the scan was partial: some folder couldn't be opened or listed.
    pub fn partial(&self) -> bool {
        !self.unscanned.is_empty()
    }
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

/// The launch-time call, before `apply`. Scans the matter; when any session of the matter is
/// open (running, or ended with no [`after_session`]), sets aside each protected name and link
/// that is new or changed since the last pass. Then drops each open session not in `running`
/// (the ids of this matter's sessions PR6 still runs): those crashed, and were just checked.
/// PR5 and PR6 call this only once a crashed session's job object is closed (kill-on-close),
/// so its agent is dead. Last, records each name and link afresh and adds `session` to the open
/// sessions. Returns what moved, for the lawyer's notice, and the folders the scan couldn't
/// open or list. On failure the launch is refused and the record stays as it was.
#[cfg(windows)]
pub fn before_launch(
    policy: &crate::Policy,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    running: &[&str],
    now: SystemTime,
) -> Result<Checked, crate::Error> {
    let root = policy.matter();
    run::before_launch(&ntfs::Ntfs, root, data_dir, matter, session, running, now)
        .map_err(run::Refused::into_error)
}

/// The call when the agent has exited, after `repair_after_session` and whatever it returned:
/// scans the matter, sets aside each protected name and link that is new or changed since the
/// last pass, whoever owns it, records them afresh and takes `session` out of the open
/// sessions. When the scan was partial, what it found is still set aside, but the session
/// stays open, so the next launch checks again. Returns what moved and the folders the scan
/// couldn't open or list. On failure the record stays as it was, so the next launch checks
/// again.
#[cfg(windows)]
pub fn after_session(
    policy: &crate::Policy,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    now: SystemTime,
) -> Result<Checked, crate::Error> {
    let root = policy.matter();
    run::after_session(&ntfs::Ntfs, root, data_dir, matter, session, now)
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
pub(super) mod tests;
// Two attributes, as in `windows.rs`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
