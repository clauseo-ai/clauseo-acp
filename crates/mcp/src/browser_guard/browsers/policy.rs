//! Windows: one DWORD of the firm's browser policy, read from the registry.
#![expect(
    unsafe_code,
    reason = "FFI to RegGetValueW, for the firm's RemoteDebuggingAllowed policy"
)]

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};
use windows::core::HSTRING;

/// Where the policy is read.
#[derive(Clone, Copy, Debug)]
pub(super) enum Hive {
    /// `HKEY_LOCAL_MACHINE`, its 64-bit view.
    Machine,
    /// `HKEY_CURRENT_USER`.
    User,
}

/// The DWORD `value` under `key`; none when it is missing, not a DWORD, or unreadable.
pub(super) fn dword(hive: Hive, key: &str, value: &str) -> Option<u32> {
    let (root, flags) = match hive {
        Hive::Machine => (HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY),
        Hive::User => (HKEY_CURRENT_USER, RRF_RT_REG_DWORD),
    };
    let mut data = 0_u32;
    let mut size = u32::try_from(size_of::<u32>()).ok()?;
    // SAFETY: the key and value names are NUL-terminated HSTRINGs live for the call; `data` is
    // a live u32 of `size` bytes the call writes, and `size` a live u32 it updates.
    let read = unsafe {
        RegGetValueW(
            root,
            &HSTRING::from(key),
            &HSTRING::from(value),
            flags,
            None,
            Some((&raw mut data).cast()),
            Some(&raw mut size),
        )
    };
    (read == ERROR_SUCCESS).then_some(data)
}
