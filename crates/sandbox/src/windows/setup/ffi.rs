//! What the set-up's Windows calls share (S15 `PR4f`): a failed call as a [`MachineError`],
//! NUL-terminated UTF-16, a token's user as a [`Sid`], the `HKLM` reads of `ProfileList`, and
//! the `ProgramData` known folder and the setup exe's DLL search (`PR4h`).
#![expect(
    unsafe_code,
    reason = "FFI to RegGetValueW, SHGetKnownFolderPath and SetDefaultDllDirectories"
)]

use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, HANDLE, WIN32_ERROR,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::LibraryLoader::{
    LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY,
    RegGetValueW,
};
use windows::Win32::UI::Shell::{FOLDERID_ProgramData, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
use windows::core::{PCWSTR, PWSTR};

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

/// A string `SHGetKnownFolderPath` allocated, freed with `CoTaskMemFree` on drop.
struct TaskString(PWSTR);

impl Drop for TaskString {
    fn drop(&mut self) {
        // SAFETY: the pointer came from SHGetKnownFolderPath (or is null, which the call
        // accepts) and is freed once, here.
        unsafe { CoTaskMemFree(Some(self.0.0.cast_const().cast())) };
    }
}

/// `%ProgramData%` as Windows knows it (`FOLDERID_ProgramData`), never from the environment,
/// which the process that started the set-up could have set.
pub(super) fn program_data() -> Result<PathBuf, MachineError> {
    let folder = FOLDERID_ProgramData;
    // SAFETY: `folder` is a live GUID for the call; no token is passed (this user's view,
    // which for ProgramData is the machine's). The string returned is owned by `TaskString`
    // at once.
    let path = unsafe { SHGetKnownFolderPath(&raw const folder, KF_FLAG_DEFAULT, None) }
        .map_err(|error| os("SHGetKnownFolderPath", &error))?;
    let path = TaskString(path);
    // SAFETY: on success the call wrote a NUL-terminated string, alive until `path` drops.
    let units = unsafe { path.0.as_wide() };
    if units.is_empty() {
        return Err(MachineError::Other(
            "Windows gave no ProgramData folder".to_owned(),
        ));
    }
    Ok(PathBuf::from(OsString::from_wide(units)))
}

/// Keeps every later DLL load of this process to System32 (`SetDefaultDllDirectories`), so a
/// DLL planted beside the setup exe, in a folder a standard user may write, never loads into the
/// elevated process. Direct static imports are kept there by the link flag
/// `/DEPENDENTLOADFLAG:0x800`. `netapi32.dll` and `userenv.dll` are delay-loaded (`build.rs`)
/// because their own imports (`netutils.dll`, `samcli.dll`, ...) aren't `KnownDLLs` and would
/// otherwise be searched for beside the exe before `main`. Called first in `exe::main`: after
/// it, the delay-load helper's `LoadLibraryExA(name, NULL, 0)` (or the loader's
/// `ResolveDelayLoadedAPI`) searches only the default directories, System32, and so do the
/// loads of those DLLs' own imports.
pub(super) fn restrict_dll_search() -> Result<(), MachineError> {
    // SAFETY: a flags-only call with no pointers.
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) }
        .map_err(|error| os("SetDefaultDllDirectories", &error))
}
