//! A pool user's profile on Windows (S15 `PR4f`): where `ProfileList` says it is, and its
//! deletion (`DeleteProfileW`). A process still running as the user holds its hive loaded and
//! makes the delete fail, so the delete first stops each process whose token user is that SID
//! (D-B13, `processes.rs`). Before the delete, every junction, symbolic link and mount point in
//! the profile, and every name of a file with another name elsewhere, is deleted itself, by
//! handle (`acl::profile::clear_links`), so the profile service's own delete meets none of them
//! (ADR 0049, "Reparse points").
#![expect(unsafe_code, reason = "FFI to DeleteProfileW")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SHARING_VIOLATION,
};
use windows::Win32::UI::Shell::DeleteProfileW;
use windows::core::PCWSTR;

use super::answers::is_temporary;
use super::ffi::{PROFILE_LIST, os, reg_dword, reg_string, wide};
use super::machine::{MachineError, ProfileDir};
use super::processes::{check_pool, stop_pool_processes};
use crate::windows::acl::privilege::{BACKUP_RESTORE, Privileges};
use crate::windows::acl::profile::{self as below, Below, clear_links};
use crate::windows::sids::Sid;

/// How many times a delete is tried while the profile service still holds the hive.
const DELETE_TRIES: u32 = 6;
/// The wait between two tries.
const DELETE_WAIT: Duration = Duration::from_millis(500);

/// The profile `ProfileList` lists for `sid`: its folder (`ProfileImagePath`, expanded) and
/// whether it is temporary; `None` when the SID has no entry.
pub fn profile_of(sid: &Sid) -> Result<Option<ProfileDir>, MachineError> {
    let key = format!(r"{PROFILE_LIST}\{}", sid.as_str());
    let Some(path) = reg_string(&key, "ProfileImagePath")? else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    let state = reg_dword(&key, "State")?;
    let temporary = is_temporary(state, &path);
    Ok(Some(ProfileDir { path, temporary }))
}

/// Deletes `sid`'s profile: its folder and its `ProfileList` entry. Refuses, deleting nothing,
/// unless `sid` passes [`check_pool`]: a local account of this machine, not built in, not this
/// process's user nor `owner` (the user who asked), and named as a pool user if it resolves.
/// Then stops every process whose token user is exactly `sid`, refusing when one can't be listed
/// or stopped; then takes the links out of the
/// folder `ProfileList` lists ([`clear_profile_links`]), refusing, deleting nothing more,
/// when it can't; then `DeleteProfileW`, tried
/// again for up to about three seconds while the profile service still holds the hive. No
/// profile is `Ok`, so a re-run skips what is gone.
pub fn delete_profile(sid: &Sid, owner: &Sid) -> Result<(), MachineError> {
    let one = std::slice::from_ref(sid);
    check_pool(one, owner)?;
    // Nothing running as the user may change the profile between the link sweep and the
    // delete: a process that can't be listed or stopped leaves the profile as it is.
    match stop_pool_processes(one, owner) {
        Ok(stopped) if !stopped.failed.is_empty() || !stopped.timed_out.is_empty() => {
            tracing::warn!(
                failed = stopped.failed.len(),
                timed_out = stopped.timed_out.len(),
                "processes of a pool user couldn't be stopped before its profile delete"
            );
            return Err(MachineError::Other(
                "a process of the sandbox user couldn't be stopped, so its profile is left as \
                 it is"
                    .to_owned(),
            ));
        }
        Ok(_) => {}
        Err(error) => {
            return Err(MachineError::Other(format!(
                "the sandbox user's processes couldn't be listed, so its profile is left as it \
                 is: {error}"
            )));
        }
    }
    if let Some(listed) = profile_of(sid)? {
        clear_profile_links(&listed.path)?;
    }
    let text = wide(sid.as_str());
    let mut tries = 0;
    loop {
        tries += 1;
        // SAFETY: the SID string is NUL-terminated and outlives the call; no profile path or
        // computer name (this one).
        let deleted =
            unsafe { DeleteProfileW(PCWSTR(text.as_ptr()), PCWSTR::null(), PCWSTR::null()) };
        let Err(error) = deleted else {
            return Ok(());
        };
        let code = error.code();
        if code == ERROR_FILE_NOT_FOUND.to_hresult() {
            return Ok(());
        }
        let busy = code == ERROR_SHARING_VIOLATION.to_hresult()
            || code == ERROR_ACCESS_DENIED.to_hresult();
        if !busy || tries >= DELETE_TRIES {
            return Err(os("DeleteProfileW", &error));
        }
        wait();
    }
}

/// [`clear_links`] on the profile folder `profile`, with the backup and restore privileges
/// enabled while it runs, so a folder the pool user shut is still listed. A folder that is gone
/// is `Ok`: `DeleteProfileW` then has only the `ProfileList` entry to delete.
fn clear_profile_links(profile: &Path) -> Result<(), MachineError> {
    let _held = backup()?;
    match clear_links(profile) {
        Ok(cleared) => {
            if cleared.links + cleared.linked_files > 0 {
                tracing::info!(
                    links = cleared.links,
                    linked_files = cleared.linked_files,
                    "links in a sandbox user's profile were deleted before its profile delete"
                );
            }
            Ok(())
        }
        Err(Below::Missing) => Ok(()),
        Err(Below::Changed(part)) => Err(MachineError::Other(format!(
            "{part} in the sandbox user's profile isn't where ProfileList puts it, or changed \
             while its links were deleted, so the profile is left as it is"
        ))),
        Err(Below::Io(error)) => Err(MachineError::Other(format!(
            "the profile's links couldn't all be deleted, so the profile is left as it is: \
             {error}"
        ))),
    }
}

/// The backup and restore privileges, enabled until the guard drops (their state before put
/// back then), for an elevated open below a profile: a folder its pool user shut to
/// administrators is still reached, with backup intent, so it can't stop a set-up or a removal.
fn backup() -> Result<Privileges, MachineError> {
    Privileges::enable(&BACKUP_RESTORE)
        .map_err(|error| MachineError::Other(format!("the backup privileges: {error}")))
}

/// The owner of `folder` in the profile `profile` (`acl::profile::owner_below`, read on the
/// handle it reaches it by), with [`backup`]'s privileges; `None` when it is missing.
pub(super) fn owner_in(profile: &Path, folder: &Path) -> Result<Option<Sid>, MachineError> {
    let _held = backup()?;
    below::owner_below(profile, folder).map_err(below_error)
}

/// Whether `folder` in the profile `profile` is there, reached by handle, with [`backup`]'s
/// privileges.
pub(super) fn reached(profile: &Path, folder: &Path) -> Result<bool, MachineError> {
    let _held = backup()?;
    match below::open_below(profile, folder) {
        Ok(_) => Ok(true),
        Err(Below::Missing) => Ok(false),
        Err(other) => Err(below_error(other)),
    }
}

/// Makes `owner` the owner of `folder` in the profile `profile` alone
/// (`acl::profile::set_owner_below`), with [`backup`]'s privileges.
pub(super) fn give(profile: &Path, folder: &Path, owner: &Sid) -> Result<(), MachineError> {
    let _held = backup()?;
    below::set_owner_below(profile, folder, owner).map_err(below_error)
}

/// A [`Below`] as the machine's error: a reparse point on the way is
/// [`MachineError::ProfileChanged`].
pub(super) fn below_error(below: Below) -> MachineError {
    match below {
        Below::Changed(part) => MachineError::ProfileChanged(part),
        Below::Missing => MachineError::Other("it isn't there".to_owned()),
        Below::Io(error) => MachineError::Other(error.to_string()),
    }
}

/// Waits [`DELETE_WAIT`] for the profile service to unload a hive.
#[expect(
    clippy::disallowed_methods,
    reason = "a bounded wait in the elevated one-shot set-up, which has no clock to inject"
)]
fn wait() {
    std::thread::sleep(DELETE_WAIT);
}
