//! The security-block mark (`security_block`) on Windows: `SecurityBlockedAt`, a `REG_QWORD`
//! `FILETIME` under `HKCU\Software\Clauseo\Sandbox`, this user's own key. It only picks which
//! message a missing admin copy shows, so nothing else reads it and its time is for support.
#![expect(unsafe_code, reason = "FFI to the Win32 registry calls")]

use std::io;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_QWORD, RRF_RT_REG_QWORD,
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows::core::{PCWSTR, w};

/// The key, under `HKEY_CURRENT_USER`.
const KEY: PCWSTR = w!(r"Software\Clauseo\Sandbox");
/// The value.
const VALUE: PCWSTR = w!("SecurityBlockedAt");

/// Whether the mark is there.
pub(crate) fn read() -> io::Result<bool> {
    let mut at = 0u64;
    let mut size = u32::try_from(size_of::<u64>()).map_err(io::Error::other)?;
    // SAFETY: the names are NUL-terminated literals; `at` is live, writable and `size` bytes
    // long for the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            KEY,
            VALUE,
            RRF_RT_REG_QWORD,
            None,
            Some((&raw mut at).cast()),
            Some(&raw mut size),
        )
    };
    present(read)
}

/// Records the mark at the time now, creating the key when it isn't there.
pub(crate) fn record() -> io::Result<()> {
    // SAFETY: no arguments; it only returns the time.
    let now = unsafe { GetSystemTimeAsFileTime() };
    let at = ((u64::from(now.dwHighDateTime) << 32) | u64::from(now.dwLowDateTime)).to_le_bytes();
    let mut key = HKEY::default();
    // SAFETY: the name is a NUL-terminated literal; `key` is a live out-pointer, and the key it
    // receives is closed below, once.
    let created = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            KEY,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &raw mut key,
            None,
        )
    };
    win32(created)?;
    // SAFETY: `key` is open, the value name a NUL-terminated literal, and `at` outlives the call.
    let set = unsafe { RegSetValueExW(key, VALUE, None, REG_QWORD, Some(&at)) };
    // SAFETY: `key` came from RegCreateKeyExW and is closed once, here. A failed close leaves
    // nothing to do.
    let _closed = unsafe { RegCloseKey(key) };
    win32(set)
}

/// Deletes the mark; a mark or key that isn't there is deleted already.
pub(crate) fn delete() -> io::Result<()> {
    // SAFETY: the names are NUL-terminated literals.
    let deleted = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, KEY, VALUE) };
    present(deleted).map(|_present| ())
}

/// `Ok(true)` for success, `Ok(false)` for a key or value that isn't there, else the error.
fn present(code: WIN32_ERROR) -> io::Result<bool> {
    match code {
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        other => win32(other).map(|()| true),
    }
}

/// `code` as a result.
fn win32(code: WIN32_ERROR) -> io::Result<()> {
    code.ok()
        .map_err(|error| io::Error::from_raw_os_error(error.code().0 & 0xFFFF))
}
