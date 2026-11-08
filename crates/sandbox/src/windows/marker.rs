//! The set-up marker, `<data_dir>\sandbox\setup.json`: which set-up version ran, for which
//! Windows user, which group and pool users it set up, and when. Set-up (S15 `PR4d`) writes it
//! last; [`read`] tells missing, older, current and another user's apart, and [`state_of`] adds
//! the machine's record (`HKLM\SOFTWARE\Clauseo\Sandbox`, [`Records`]) to give the
//! [`SetupState`].
//!
//! Built on every OS so its tests run everywhere; only Windows uses it.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::matters::SANDBOX_DIR;
use super::sids::Sid;
use super::write;
use crate::{SetupError, SetupState};

/// The marker's file, in the sandbox folder.
pub const SETUP_FILE: &str = "setup.json";
/// The set-up version this build makes. A marker below it means set-up must run again. Version 1
/// named one `ClauseoSandbox` account; version 2 names a group and a pool of users (D9, D-B8).
pub const SETUP_VERSION: u32 = 2;

/// What `setup.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    /// The set-up version that wrote it.
    pub version: u32,
    /// The Windows user who ran the set-up and owns it (D5).
    pub user_sid: Sid,
    /// The `ClauseoSandboxUsers` group the set-up made or adopted.
    pub group_sid: Sid,
    /// Every pool user the set-up made or reset, as it recorded them in `PoolSids`.
    pub pool: Vec<Sid>,
    /// When the set-up finished, in seconds since the Unix epoch (set-up passes the time in).
    pub set_up_at: u64,
}

/// What [`read`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerState {
    /// No `setup.json`: never set up here.
    Missing,
    /// Written by an older set-up, for this user.
    Older {
        /// Its version.
        found: u32,
    },
    /// Written by this set-up version, for this user.
    Current(Marker),
    /// Written for another Windows user, at any version.
    OtherUser {
        /// The user it names.
        user_sid: Sid,
    },
    /// Written by a newer set-up, for this user; this build can't vouch for it.
    Newer {
        /// Its version.
        found: u32,
    },
    /// Not readable, or not a marker; why.
    Unreadable(String),
}

/// The fields every version of the marker has, read before the version decides the rest.
#[derive(Deserialize)]
struct Head {
    version: u32,
    user_sid: Sid,
}

/// The marker's path under `data_dir`.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(SANDBOX_DIR).join(SETUP_FILE)
}

/// Reads `<data_dir>\sandbox\setup.json` for the Windows user `user`, after
/// `write::check_owned_file` (a link, a second hard link or an owner that can't be taken back is
/// [`MarkerState::Unreadable`]). The user is checked
/// before the version, so another user's marker is [`MarkerState::OtherUser`] whatever its
/// version; then a lower version is [`MarkerState::Older`], a higher one
/// [`MarkerState::Newer`], and this version must parse whole to be [`MarkerState::Current`].
pub fn read(data_dir: &Path, user: &Sid) -> MarkerState {
    let file = path(data_dir);
    if let Err(error) = write::check_owned_file(&file) {
        return MarkerState::Unreadable(error.to_string());
    }
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return MarkerState::Missing,
        Err(error) => return MarkerState::Unreadable(error.to_string()),
    };
    let head: Head = match serde_json::from_slice(&bytes) {
        Ok(head) => head,
        Err(error) => return MarkerState::Unreadable(error.to_string()),
    };
    if head.user_sid != *user {
        return MarkerState::OtherUser {
            user_sid: head.user_sid,
        };
    }
    match head.version {
        found if found < SETUP_VERSION => MarkerState::Older { found },
        found if found > SETUP_VERSION => MarkerState::Newer { found },
        _ => match serde_json::from_slice(&bytes) {
            Ok(marker) => MarkerState::Current(marker),
            Err(error) => MarkerState::Unreadable(error.to_string()),
        },
    }
}

/// Writes `marker` as `<data_dir>\sandbox\setup.json`, atomically, making the folder if
/// missing and checking the old file first (`write::check_owned_file`), and the new one after.
/// Set-up's last step (S15 PR4).
pub fn write(data_dir: &Path, marker: &Marker) -> Result<(), SetupError> {
    let file = path(data_dir);
    let failed = |why: String| SetupError::Failed(format!("{}: {why}", file.display()));
    let mut bytes = serde_json::to_vec_pretty(marker).map_err(|error| failed(error.to_string()))?;
    bytes.push(b'\n');
    if let Some(folder) = file.parent() {
        write::make_sandbox_dir(folder).map_err(|error| failed(error.to_string()))?;
    }
    write::check_owned_file(&file).map_err(|error| failed(error.to_string()))?;
    write::write_atomic(&file, &bytes).map_err(|error| failed(error.to_string()))?;
    // The new file in place gets the same explicit DACL.
    write::check_owned_file(&file).map_err(|error| failed(error.to_string()))
}

/// The machine's record, `HKLM\SOFTWARE\Clauseo\Sandbox` (`pool.rs`'s `MachineRecord`), as
/// the set-up state reads it: a seam, so tests give each answer. `Err` says why a value can't
/// be read.
pub trait Records {
    /// `OwnerSid`: the Windows user who set the pool up. Data folders are per user, so this
    /// machine-wide value is how a second user learns that the first owns the set-up (D5).
    /// `Ok(None)` when missing.
    fn owner(&self) -> Result<Option<String>, String>;
    /// `GroupSid`: the `ClauseoSandboxUsers` group's SID. `Ok(None)` when missing.
    fn group(&self) -> Result<Option<String>, String>;
    /// `PoolSids`: every pool user's SID, each checked; empty when missing. A malformed list is
    /// `Err`, never a shorter list.
    fn pool(&self) -> Result<Vec<Sid>, String>;
    /// `Removing`: a removal started and has not finished. `Ok(false)` when missing.
    fn removing(&self) -> Result<bool, String>;
}

/// The real records, read through `record.rs`.
#[cfg(windows)]
pub struct MachineRecords;

#[cfg(windows)]
impl Records for MachineRecords {
    fn owner(&self) -> Result<Option<String>, String> {
        super::record::sandbox_owner_sid().map_err(|error| error.to_string())
    }

    fn group(&self) -> Result<Option<String>, String> {
        super::record::sandbox_group_sid().map_err(|error| error.to_string())
    }

    fn pool(&self) -> Result<Vec<Sid>, String> {
        super::record::sandbox_pool_sids().map_err(|error| error.to_string())
    }

    fn removing(&self) -> Result<bool, String> {
        super::record::sandbox_removing().map_err(|error| error.to_string())
    }
}

/// What a set `Removing` gives: the removal stopped part way, and only an elevated run can
/// finish or undo it.
pub const REMOVAL_UNFINISHED: &str =
    "removal unfinished: reinstall and uninstall, or run set-up again";

/// The set-up state of the Windows user `user` from the marker and the machine's `records`.
///
/// - First `OwnerSid`: when it names another user than `user`, [`SetupState::SetUpByOtherUser`]
///   (D5) whatever the rest says; when it can't be read or is not a SID, [`SetupState::Failed`].
/// - Then `Removing`: set, [`SetupState::Failed`] with [`REMOVAL_UNFINISHED`], whatever the
///   marker says; unreadable, [`SetupState::Failed`].
/// - Missing marker: [`SetupState::Needed`]. Older: [`SetupState::Outdated`]. Another user's:
///   [`SetupState::SetUpByOtherUser`] (D5). Newer or unreadable: [`SetupState::Failed`].
/// - Current: [`SetupState::Ready`] only when `OwnerSid` is `user`, `GroupSid` is the marker's
///   group and every pool SID the marker names is in `PoolSids` (canonical strings, so equal
///   strings are equal SIDs). `PoolSids` may hold more: a later set-up that grew the pool and
///   stopped before its marker. No `OwnerSid`, another or missing group, or a marker pool SID
///   missing from the record: the record was removed or rewritten, [`SetupState::Needed`]. A
///   marker with an empty pool, or a value that can't be read or is not a SID:
///   [`SetupState::Failed`].
pub fn state_of(marker: MarkerState, user: &Sid, records: &impl Records) -> SetupState {
    let owned = match records.owner() {
        Ok(None) => false,
        Ok(Some(text)) => match Sid::parse(&text) {
            Ok(owner) if owner == *user => true,
            Ok(_) => return SetupState::SetUpByOtherUser,
            Err(error) => return SetupState::Failed(format!("the sandbox owner record: {error}")),
        },
        Err(why) => return SetupState::Failed(format!("the sandbox owner record: {why}")),
    };
    match records.removing() {
        Ok(false) => {}
        Ok(true) => return SetupState::Failed(REMOVAL_UNFINISHED.to_owned()),
        Err(why) => return SetupState::Failed(format!("the sandbox removal record: {why}")),
    }
    let marker = match marker {
        MarkerState::Missing => return SetupState::Needed,
        MarkerState::Older { found } => {
            return SetupState::Outdated {
                found,
                current: SETUP_VERSION,
            };
        }
        MarkerState::OtherUser { .. } => return SetupState::SetUpByOtherUser,
        MarkerState::Newer { found } => {
            return SetupState::Failed(format!(
                "{SETUP_FILE} is from a newer Clauseo (version {found}, this one sets up \
                 {SETUP_VERSION})"
            ));
        }
        MarkerState::Unreadable(why) => return SetupState::Failed(format!("{SETUP_FILE}: {why}")),
        MarkerState::Current(marker) => marker,
    };
    if marker.pool.is_empty() {
        return SetupState::Failed(format!("{SETUP_FILE} names no pool user"));
    }
    let group = match records.group() {
        Ok(None) => None,
        Ok(Some(text)) => match Sid::parse(&text) {
            Ok(group) => Some(group),
            Err(error) => return SetupState::Failed(format!("the sandbox group record: {error}")),
        },
        Err(why) => return SetupState::Failed(format!("the sandbox group record: {why}")),
    };
    let pool = match records.pool() {
        Ok(pool) => pool,
        Err(why) => return SetupState::Failed(format!("the sandbox pool record: {why}")),
    };
    let recorded = marker.pool.iter().all(|sid| pool.contains(sid));
    if owned && group.as_ref() == Some(&marker.group_sid) && recorded {
        SetupState::Ready
    } else {
        SetupState::Needed
    }
}

/// This Windows user's set-up state on this machine: `setup.json` under `data_dir`, read for
/// this process's user, and the machine's record `HKLM\SOFTWARE\Clauseo\Sandbox`. What
/// S15 PR4's `UserSetup::state` returns; reads only, every call.
#[cfg(windows)]
pub fn setup_state(data_dir: &Path) -> SetupState {
    let user = match super::record::current_user_sid() {
        Ok(user) => user,
        Err(error) => return SetupState::Failed(format!("this user's SID: {error}")),
    };
    state_of(read(data_dir, &user), &user, &MachineRecords)
}

#[cfg(test)]
mod tests;
