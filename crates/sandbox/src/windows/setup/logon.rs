//! The set-up's one logon per pool user on Windows (S15 `PR4f`, step 6): `LogonUserW`
//! (`LOGON32_LOGON_INTERACTIVE`), then `LoadUserProfileW`, which makes the profile from Default
//! the first time (`AppData\LocalLow` included), then `UnloadUserProfile`. A temporary profile
//! is reported ([`ProfileDir::temporary`]), so the engine fails rather than close the wrong
//! `LocalLow` (K5).
//!
//! The password goes to `LogonUserW` from a buffer wiped on drop; no error, log or `Debug`
//! output carries it.
#![expect(
    unsafe_code,
    reason = "FFI to LogonUserW and the userenv profile calls"
)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, HANDLE};
use windows::Win32::Security::{LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LogonUserW};
use windows::Win32::UI::Shell::{
    GetUserProfileDirectoryW, LoadUserProfileW, PROFILEINFOW, UnloadUserProfile,
};
use windows::core::{PCWSTR, PWSTR, w};
use zeroize::Zeroizing;

use super::answers::{is_temporary, logon_error};
use super::ffi::{PROFILE_LIST, code_of, os, reg_dword, token_user, wide};
use super::machine::{MachineError, Password, ProfileDir};
use crate::windows::acl::object::Handle;

/// `PI_NOUI`: the profile load shows no message box.
const PI_NOUI: u32 = 1;

/// Logs the local account `name` on once with `password`, its profile loaded, then unloads it;
/// the profile's folder, and whether Windows gave a temporary one. A refused logon is
/// [`MachineError::Os`] from `LogonUserW` with its code: 1385 when "Allow log on locally"
/// doesn't include the account, 1326 for a wrong password.
pub fn log_on_once(name: &str, password: &Password) -> Result<ProfileDir, MachineError> {
    let mut user = wide(name);
    // Sized once, so no reallocation leaves an unwiped copy of the password behind.
    let units = password.encode_utf16().count();
    let mut secret: Zeroizing<Vec<u16>> = Zeroizing::new(Vec::with_capacity(units + 1));
    secret.extend(password.encode_utf16());
    secret.push(0);
    let mut token = HANDLE::default();
    // SAFETY: the name, the domain (`.`, this computer) and the password are NUL-terminated and
    // outlive the call; `token` is a live out-pointer, owned by `Handle` once written.
    let logged_on = unsafe {
        LogonUserW(
            PCWSTR(user.as_ptr()),
            w!("."),
            PCWSTR(secret.as_ptr()),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &raw mut token,
        )
    };
    drop(secret);
    if let Err(error) = logged_on {
        return Err(logon_error(code_of(&error)));
    }
    let token = Handle(token);
    let mut info = PROFILEINFOW {
        dwSize: u32::try_from(size_of::<PROFILEINFOW>()).unwrap_or(0),
        dwFlags: PI_NOUI,
        lpUserName: PWSTR(user.as_mut_ptr()),
        ..PROFILEINFOW::default()
    };
    // SAFETY: the token is a live logon token; `info` is sized and its name points into
    // `user`, which outlives the call; the loaded profile is unloaded by `Loaded`.
    unsafe { LoadUserProfileW(token.0, &raw mut info) }
        .map_err(|error| os("LoadUserProfileW", &error))?;
    let loaded = Loaded {
        token: token.0,
        profile: info.hProfile,
    };
    let path = profile_directory(token.0)?;
    let sid = token_user(token.0)?;
    let state = match &sid {
        Some(sid) => reg_dword(&format!(r"{PROFILE_LIST}\{}", sid.as_str()), "State")?,
        None => None,
    };
    drop(loaded);
    drop(token);
    let temporary = is_temporary(state, &path);
    Ok(ProfileDir { path, temporary })
}

/// The profile folder of `token`'s user (`GetUserProfileDirectoryW`).
fn profile_directory(token: HANDLE) -> Result<PathBuf, MachineError> {
    let mut size = 0u32;
    // SAFETY: no buffer asks only for the size, written to `size`.
    match unsafe { GetUserProfileDirectoryW(token, None, &raw mut size) } {
        Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {}
        Err(error) => return Err(os("GetUserProfileDirectoryW", &error)),
        Ok(()) => return Err(MachineError::Other("the profile folder is empty".into())),
    }
    let mut buffer = vec![0u16; usize::try_from(size).unwrap_or(0).max(1)];
    // SAFETY: `buffer` is live and writable for `size` characters.
    unsafe { GetUserProfileDirectoryW(token, Some(PWSTR(buffer.as_mut_ptr())), &raw mut size) }
        .map_err(|error| os("GetUserProfileDirectoryW", &error))?;
    let end = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    buffer.truncate(end);
    Ok(PathBuf::from(OsString::from_wide(&buffer)))
}

/// A loaded profile, unloaded on drop (before its token closes).
struct Loaded {
    token: HANDLE,
    profile: HANDLE,
}

impl Drop for Loaded {
    fn drop(&mut self) {
        // SAFETY: the token is still open (it is dropped after this guard) and `profile` came
        // from LoadUserProfileW for it; unloaded once, here.
        if let Err(error) = unsafe { UnloadUserProfile(self.token, self.profile) } {
            tracing::warn!(code = error.code().0, "UnloadUserProfile failed");
        }
    }
}

#[cfg(test)]
mod tests;
