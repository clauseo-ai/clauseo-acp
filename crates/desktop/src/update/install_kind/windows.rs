//! Windows: whether the per-machine installer's uninstall key is in the machine's registry.
#![expect(unsafe_code, reason = "FFI to the Win32 registry")]

use std::io;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, RegCloseKey, RegOpenKeyExW,
};
use windows::core::w;

/// Whether `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Clauseo` exists in the
/// 64-bit view, which the per-machine installer writes (`docs/firm-it.md`, section 1) and the
/// per-user one never does (it writes `HKCU`'s). Everyone may read it. Any error but a missing
/// key is the error.
pub(super) fn uninstall_key_exists() -> io::Result<bool> {
    let mut key = HKEY::default();
    // SAFETY: the subkey is a NUL-terminated literal and `key` a live out-pointer for the call.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Clauseo"),
            Some(0),
            KEY_READ | KEY_WOW64_64KEY,
            &raw mut key,
        )
    };
    match opened {
        ERROR_SUCCESS => {
            // SAFETY: `key` was opened above and is closed once, here. A failed close leaves
            // nothing to do.
            let _closed = unsafe { RegCloseKey(key) };
            Ok(true)
        }
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        other => Err(io::Error::from_raw_os_error(
            i32::try_from(other.0).unwrap_or(i32::MAX),
        )),
    }
}
