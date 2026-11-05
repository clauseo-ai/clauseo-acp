//! The download's Win32 calls: System32's folder (for `curl.exe` and `tar.exe`), the free disk
//! space, and the Authenticode check of `chrome.exe`.

#![expect(
    unsafe_code,
    reason = "GetSystemDirectoryW, GetDiskFreeSpaceExW and WinVerifyTrust, the Win32 calls the \
              windows crate binds; each call's buffers and structs live across it"
)]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::core::PCWSTR;

/// The longest System32 path read; `MAX_PATH` is 260.
const MAX_SYSTEM_DIR: usize = 1024;

/// `path` as a NUL-terminated wide string.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// The System32 folder, from `GetSystemDirectoryW`, never from a variable.
pub(super) fn system_dir() -> Option<PathBuf> {
    let mut buffer = vec![0u16; MAX_SYSTEM_DIR];
    // SAFETY: the call writes at most `buffer.len()` units into the live `buffer`.
    let len = unsafe { GetSystemDirectoryW(Some(&mut buffer)) };
    let len = usize::try_from(len)
        .ok()
        .filter(|&len| len > 0 && len < buffer.len())?;
    buffer.truncate(len);
    Some(PathBuf::from(OsString::from_wide(&buffer)))
}

/// The bytes free to this user on the volume that holds `folder`.
pub(super) fn free_bytes(folder: &Path) -> io::Result<u64> {
    let name = wide(folder);
    let mut free = 0u64;
    // SAFETY: `name` is a live NUL-terminated wide string and `free` a live u64; the other two
    // outputs are null, which the call allows.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(name.as_ptr()), Some(&raw mut free), None, None) }
        .map_err(io::Error::other)?;
    Ok(free)
}

/// Authenticode on `file` (`WinVerifyTrust`, the generic verify action, no UI, no revocation
/// fetch): `Err` with the call's result when it does not pass.
pub(super) fn verify_authenticode(file: &Path) -> Result<(), String> {
    let name = wide(file);
    let mut info = WINTRUST_FILE_INFO {
        cbStruct: size_u32::<WINTRUST_FILE_INFO>(),
        pcwszFilePath: PCWSTR(name.as_ptr()),
        hFile: HANDLE::default(),
        pgKnownSubject: std::ptr::null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_u32::<WINTRUST_DATA>(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &raw mut info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: `data` points at the live `info`, which points at the live `name`; all three
    // outlive both calls. The second call closes the state the first one opened.
    let status = unsafe {
        let status = WinVerifyTrust(HWND::default(), &raw mut action, (&raw mut data).cast());
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(HWND::default(), &raw mut action, (&raw mut data).cast());
        status
    };
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "WinVerifyTrust returned {status:#010x} for {}",
            file.display()
        ))
    }
}

/// The size of `T` as the `cbStruct` field takes it.
fn size_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}
