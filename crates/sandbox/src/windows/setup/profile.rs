//! A pool user's profile on Windows (S15 `PR4f`): where `ProfileList` says it is, and its
//! deletion (`DeleteProfileW`). A process still running as the user holds its hive loaded and
//! makes the delete fail, so the delete first stops each process whose token user is that SID
//! (D-B13, `processes.rs`).
#![expect(unsafe_code, reason = "FFI to DeleteProfileW")]

use std::path::PathBuf;
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
/// unless `sid` passes [`check_pool`]: a local account of this machine, not built in, and not
/// this process's user. Then stops every process whose token user is exactly `sid`; then
/// `DeleteProfileW`, tried again for up to about three seconds while the profile service still
/// holds the hive. No profile is `Ok`, so a re-run
/// skips what is gone.
pub fn delete_profile(sid: &Sid) -> Result<(), MachineError> {
    let one = std::slice::from_ref(sid);
    check_pool(one)?;
    // A stop that fails still leaves the delete to try: it fails, named, if the hive is held.
    match stop_pool_processes(one) {
        Ok(stopped) if !stopped.failed.is_empty() || !stopped.timed_out.is_empty() => {
            tracing::warn!(
                failed = stopped.failed.len(),
                timed_out = stopped.timed_out.len(),
                "processes of a pool user couldn't be stopped before its profile delete"
            );
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "the processes of a pool user couldn't be listed"),
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

/// Waits [`DELETE_WAIT`] for the profile service to unload a hive.
#[expect(
    clippy::disallowed_methods,
    reason = "a bounded wait in the elevated one-shot set-up, which has no clock to inject"
)]
fn wait() {
    std::thread::sleep(DELETE_WAIT);
}
