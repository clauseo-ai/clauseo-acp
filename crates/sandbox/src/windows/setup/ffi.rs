//! What the set-up's Windows calls share (S15 `PR4f`): a failed call as a [`MachineError`],
//! NUL-terminated UTF-16, a token's user as a [`Sid`], and the `HKLM` reads of `ProfileList`.
#![expect(unsafe_code, reason = "FFI to RegGetValueW")]

use std::ffi::c_void;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, HANDLE, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    RegGetValueW,
};
use windows::core::PCWSTR;

use super::machine::MachineError;
use crate::windows::record::user_of;
use crate::windows::sids::Sid;

/// `HKLM\...\ProfileList`: one subkey per profile, named by its SID.
pub(super) const PROFILE_LIST: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";

/// `call` failed with `error`: its Win32 code, as [`MachineError::Os`].
pub(super) fn os(call: &'static str, error: &windows::core::Error) -> MachineError {
    MachineError::Os {
        call,
        code: code_of(error),
    }
}

/// The Win32 code `error` carries.
pub(super) fn code_of(error: &windows::core::Error) -> u32 {
    u32::try_from(error.code().0 & 0xFFFF).unwrap_or(u32::MAX)
}

/// `call` failed with the Win32 code `code`.
pub(super) fn os_code(call: &'static str, code: WIN32_ERROR) -> MachineError {
    MachineError::Os { call, code: code.0 }
}

/// `text` as NUL-terminated UTF-16.
pub(super) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// `token`'s user; `None` when its SID text isn't in [`Sid`]'s canonical form (which no SID
/// `ConvertSidToStringSidW` writes fails).
pub(super) fn token_user(token: HANDLE) -> Result<Option<Sid>, MachineError> {
    let text = user_of(token).map_err(|error| MachineError::Os {
        call: "GetTokenInformation",
        code: error
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok())
            .unwrap_or(u32::MAX),
    })?;
    Ok(Sid::parse(&text).ok())
}

/// The `REG_SZ` or `REG_EXPAND_SZ` (expanded) `value` of `HKLM\<subkey>`, in the 64-bit view;
/// `None` when the key or the value is missing.
pub(super) fn reg_string(subkey: &str, value: &str) -> Result<Option<String>, MachineError> {
    let (subkey, value) = (wide(subkey), wide(value));
    let mut size = 0u32;
    // SAFETY: the names are NUL-terminated and outlive the call; no data pointer asks only for
    // the size, written to `size`.
    let found = unsafe { get(&subkey, &value, RRF_RT_REG_SZ, None, &raw mut size) };
    if !present(found)? {
        return Ok(None);
    }
    let units = usize::try_from(size).unwrap_or(0).div_ceil(2).max(1);
    let mut buffer = vec![0u16; units];
    let mut written = u32::try_from(size_of_val(buffer.as_slice())).unwrap_or(u32::MAX);
    // SAFETY: the names are as above; `buffer` is live, writable and `written` bytes long.
    let read = unsafe {
        get(
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            Some(buffer.as_mut_ptr().cast()),
            &raw mut written,
        )
    };
    if !present(read)? {
        return Ok(None);
    }
    let text = buffer.split(|unit| *unit == 0).next().unwrap_or_default();
    String::from_utf16(text)
        .map(Some)
        .map_err(|_not_utf16| MachineError::Other("a registry value is not text".to_owned()))
}

/// The `REG_DWORD` `value` of `HKLM\<subkey>`, in the 64-bit view; `None` when missing.
pub(super) fn reg_dword(subkey: &str, value: &str) -> Result<Option<u32>, MachineError> {
    let (subkey, value) = (wide(subkey), wide(value));
    let mut answer = 0u32;
    let mut size = u32::try_from(size_of::<u32>()).unwrap_or(4);
    // SAFETY: the names are NUL-terminated and outlive the call; `answer` is live, writable and
    // `size` bytes long.
    let read = unsafe {
        get(
            &subkey,
            &value,
            RRF_RT_REG_DWORD,
            Some((&raw mut answer).cast()),
            &raw mut size,
        )
    };
    Ok(present(read)?.then_some(answer))
}

/// `RegGetValueW` on `HKLM\<subkey>` `value` in the 64-bit view, restricted to `kind`.
///
/// # Safety
///
/// `subkey` and `value` are NUL-terminated; `data`, when given, is writable for `*size` bytes;
/// `size` is a live pointer.
unsafe fn get(
    subkey: &[u16],
    value: &[u16],
    kind: REG_ROUTINE_FLAGS,
    data: Option<*mut c_void>,
    size: *mut u32,
) -> WIN32_ERROR {
    // SAFETY: as the function's contract says.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            kind | RRF_SUBKEY_WOW6464KEY,
            None,
            data,
            Some(size),
        )
    }
}

/// Whether a `RegGetValueW` result read the value (`true`) or found nothing (`false`).
fn present(code: WIN32_ERROR) -> Result<bool, MachineError> {
    match code {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        other => Err(os_code("RegGetValueW", other)),
    }
}
