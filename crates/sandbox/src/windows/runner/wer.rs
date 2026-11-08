//! Windows Error Reporting off for the pool user (decision 0049, "WER"): the inner runner, as
//! the pool user and so owning its `HKCU`, sets `Disabled` and `DontShowUI` to 1 under
//! `HKCU\Software\Microsoft\Windows\Windows Error Reporting` before the agent starts. The
//! second of two controls: the job's `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION`
//! (`spawn::JOB_LIMITS`) is the first, ending a crashed process before WER is asked. Per-user
//! `LocalDumps` settings don't exist (Windows reads `LocalDumps` from `HKLM` only), so none is
//! set. Setting the values again is a no-op; a failure is logged and the run goes on.
#![expect(unsafe_code, reason = "FFI to the registry calls")]

use std::io;

use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, RegCloseKey,
    RegCreateKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

/// The per-user WER key, under `HKEY_CURRENT_USER`.
pub(super) const WER_KEY: &str = r"Software\Microsoft\Windows\Windows Error Reporting";

/// The DWORD values set under [`WER_KEY`]: no reporting, and no crash dialog.
pub(super) const WER_VALUES: [(&str, u32); 2] = [("Disabled", 1), ("DontShowUI", 1)];

/// Sets [`WER_VALUES`] under `HKEY_CURRENT_USER\<key>`, making the key if it is missing.
pub(super) fn quiet(key: &str) -> io::Result<()> {
    let subkey = HSTRING::from(key);
    let mut opened = HKEY::default();
    // SAFETY: `subkey` is a live NUL-terminated string; no class, attributes or disposition;
    // `opened` is a live out-pointer, owned by `Key` once written.
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &raw mut opened,
            None,
        )
    };
    check(code)?;
    let opened = Key(opened);
    for (name, value) in WER_VALUES {
        let name = HSTRING::from(name);
        // SAFETY: the key is open for setting values; the data is the DWORD's four bytes.
        let code =
            unsafe { RegSetValueExW(opened.0, &name, None, REG_DWORD, Some(&value.to_le_bytes())) };
        check(code)?;
    }
    Ok(())
}

/// The log line for a failed [`quiet`].
pub(super) fn failed_line(error: &io::Error) -> String {
    let code = error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |c| c.to_string());
    format!(
        "clauseo-sandbox-run: turning off Windows Error Reporting for the sandbox user failed ({code})"
    )
}

fn check(code: WIN32_ERROR) -> io::Result<()> {
    if code == ERROR_SUCCESS {
        return Ok(());
    }
    Err(io::Error::from_raw_os_error(
        i32::try_from(code.0).unwrap_or(i32::MAX),
    ))
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: made by RegCreateKeyExW and closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

#[cfg(test)]
mod tests;
