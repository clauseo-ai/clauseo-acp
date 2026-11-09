//! The machine's record of the sandbox pool, and this process's own user: what the set-up state
//! and the desktop's start-up check read. The record is `HKLM\SOFTWARE\Clauseo\Sandbox` (D-B1),
//! with the values [`MachineRecord`](super::pool::MachineRecord) lists: `OwnerSid`, `GroupSid`,
//! `PoolSids`, `DataDir` and `Removing`. S15's elevated set-up writes it (`PR4e`) and #317's
//! check reads it before settings or logging, so its place does not depend on the data folder.
//!
//! The key's DACL, which `PR4e`'s writer sets, is `D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)`:
//! protected, full control for SYSTEM and Administrators, read for Users. Users must read it
//! because #317 runs under a pool token, whose restricting set holds `Users`. No reader here
//! writes or creates the key.
#![expect(unsafe_code, reason = "FFI to the Win32 registry, token and SID calls")]

use std::io;
use std::path::PathBuf;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER,
    ERROR_NO_TOKEN, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, ERROR_UNSUPPORTED_TYPE, HANDLE, HLOCAL,
    LocalFree, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    GetLengthSid, GetTokenInformation, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_QUERY,
    TOKEN_USER, TokenGroups, TokenUser,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_ROUTINE_FLAGS, RRF_NOEXPAND, RRF_RT_REG_DWORD, RRF_RT_REG_MULTI_SZ,
    RRF_RT_REG_QWORD, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};
use windows::core::{HRESULT, PCWSTR, PWSTR, w};

use super::pool::{MultiSzError, pool_sids_from_multi_sz};
use super::sids::Sid;

/// The record's key under `HKLM`. The only place that names it.
const RECORD_KEY: PCWSTR = w!("SOFTWARE\\Clauseo\\Sandbox");

/// Why a registry read failed. The desktop's check (#317) counts all but `Other` as sandboxed.
#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    /// `ERROR_ACCESS_DENIED`.
    #[error("the sandbox record can't be read: access denied")]
    Denied,
    /// The value has another type than the reader asks for (a `REG_EXPAND_SZ` for a `REG_SZ`
    /// included).
    #[error("the sandbox record value has the wrong type")]
    WrongType,
    /// The value is not UTF-16.
    #[error("the sandbox record value is not text")]
    NotText,
    /// Anything else.
    #[error("the sandbox record can't be read: {0}")]
    Other(io::Error),
    /// `PoolSids` is not a list of SIDs (`pool_sids_from_multi_sz`).
    #[error("the sandbox pool record is malformed: {0}")]
    Malformed(MultiSzError),
}

/// Every pool user's SID, as S15's set-up records it: value `PoolSids`, a `REG_MULTI_SZ`, in
/// the 64-bit view, readable by everyone. `Ok(vec![])` when the key or the value is missing (no
/// pool here) or the list is empty. Each string is checked as a SID, and a malformed list
/// (`pool_sids_from_multi_sz`) is [`RecordError::Malformed`]: never a shorter list. The
/// desktop's check (#317) compares the token user with each.
pub fn sandbox_pool_sids() -> Result<Vec<Sid>, RecordError> {
    let Some(units) = read_units(RECORD_KEY, w!("PoolSids"), RRF_RT_REG_MULTI_SZ)? else {
        return Ok(Vec::new());
    };
    pool_sids_from_multi_sz(&units).map_err(RecordError::Malformed)
}

/// The Windows user who set the sandbox up: value `OwnerSid` (`REG_SZ`, the SID string, 64-bit
/// view), written by the set-up with the setting-up user's SID. `Ok(None)` when missing. The
/// text comes back as stored, unchecked.
pub fn sandbox_owner_sid() -> Result<Option<String>, RecordError> {
    read_string(RECORD_KEY, w!("OwnerSid"))
}

/// The `ClauseoSandboxUsers` group's SID: value `GroupSid` (`REG_SZ`, 64-bit view).
/// `Ok(None)` when missing. Unchecked text, as [`sandbox_owner_sid`].
pub fn sandbox_group_sid() -> Result<Option<String>, RecordError> {
    read_string(RECORD_KEY, w!("GroupSid"))
}

/// What set-up or the SYSTEM refresh last found policy says of "Allow log on locally": value
/// `LogonPolicy` (`REG_SZ`, `setup::rights_policy::PolicyState::text`, 64-bit view), which the
/// desktop user can read where the policy itself it can't. `Ok(None)` when missing. Unchecked.
pub(crate) fn sandbox_logon_policy() -> Result<Option<String>, RecordError> {
    read_string(RECORD_KEY, w!("LogonPolicy"))
}

/// The owner's data folder, which holds `sandbox\`: value `DataDir` (`REG_SZ`, read
/// unexpanded, 64-bit view). `Ok(None)` when missing. Unchecked.
pub fn sandbox_data_dir() -> Result<Option<PathBuf>, RecordError> {
    Ok(read_string(RECORD_KEY, w!("DataDir"))?.map(PathBuf::from))
}

/// Whether a removal started and has not finished: value `Removing` (`REG_DWORD`), set when
/// it is non-zero. `Ok(false)` when missing.
pub(crate) fn sandbox_removing() -> Result<bool, RecordError> {
    read_dword(RECORD_KEY, w!("Removing")).map(|value| value.is_some_and(|value| value != 0))
}

/// The admin copy's SHA-256 as the installer's `--adopt` recorded it (`AdminCopySha256`,
/// `windows::admin_copy`); `Ok(None)` when the key or the value is missing.
pub(crate) fn admin_copy_sha256() -> Result<Option<String>, RecordError> {
    read_string(RECORD_KEY, w!("AdminCopySha256"))
}

/// The install folder `--adopt` was given (`UserInstallDir`, read unexpanded), the folder the
/// refresh task copies from; `Ok(None)` when the key or the value is missing. Untrusted.
pub(crate) fn user_install_dir() -> Result<Option<PathBuf>, RecordError> {
    Ok(read_string(RECORD_KEY, w!("UserInstallDir"))?.map(PathBuf::from))
}

/// The admin copy's owner as `--adopt` recorded it (`AdminCopyOwner`: a SID, or `machine`);
/// `Ok(None)` when the key or the value is missing.
pub(crate) fn admin_copy_owner() -> Result<Option<String>, RecordError> {
    read_string(RECORD_KEY, w!("AdminCopyOwner"))
}

/// The `REG_SZ` `value` of `HKLM\<subkey>`, read in the 64-bit view (`RRF_SUBKEY_WOW6464KEY`,
/// `RegGetValueW`'s `KEY_WOW64_64KEY`), up to its first NUL. Missing key or value: `Ok(None)`.
/// Another type, a `REG_EXPAND_SZ` included (refused unexpanded, `RRF_NOEXPAND`):
/// [`RecordError::WrongType`]. Refused: [`RecordError::Denied`]. Not UTF-16:
/// [`RecordError::NotText`].
pub(crate) fn read_string(subkey: PCWSTR, value: PCWSTR) -> Result<Option<String>, RecordError> {
    let Some(units) = read_units(subkey, value, RRF_RT_REG_SZ)? else {
        return Ok(None);
    };
    let text = units.split(|unit| *unit == 0).next().unwrap_or_default();
    String::from_utf16(text)
        .map(Some)
        .map_err(|_not_utf16| RecordError::NotText)
}

/// The UTF-16 units of the string-typed `value` of `HKLM\<subkey>` whose type `kind` names,
/// as [`read_string`] reads (64-bit view, unexpanded), with every NUL `RegGetValueW` wrote.
pub(crate) fn read_units(
    subkey: PCWSTR,
    value: PCWSTR,
    kind: REG_ROUTINE_FLAGS,
) -> Result<Option<Vec<u16>>, RecordError> {
    let flags = kind | RRF_NOEXPAND | RRF_SUBKEY_WOW6464KEY;
    let mut size = 0u32;
    // SAFETY: the key and value names are NUL-terminated literals; no data pointer asks only for
    // the size, written to `size`.
    let found = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            flags,
            None,
            None,
            Some(&raw mut size),
        )
    };
    if !present(found)? {
        return Ok(None);
    }
    let units = usize::try_from(size)
        .map_err(|error| RecordError::Other(io::Error::other(error)))?
        .div_ceil(2)
        .max(1);
    let mut buffer = vec![0u16; units];
    let mut written = u32::try_from(size_of_val(buffer.as_slice()))
        .map_err(|error| RecordError::Other(io::Error::other(error)))?;
    // SAFETY: `buffer` is live, writable and `written` bytes long for the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut written),
        )
    };
    if !present(read)? {
        return Ok(None);
    }
    let units = usize::try_from(written / 2).map_or(0, |n| n.min(buffer.len()));
    buffer.truncate(units);
    Ok(Some(buffer))
}

/// The `REG_DWORD` `value` of `HKLM\<subkey>`, in the 64-bit view. Missing: `Ok(None)`.
/// Another type: [`RecordError::WrongType`].
pub(crate) fn read_dword(subkey: PCWSTR, value: PCWSTR) -> Result<Option<u32>, RecordError> {
    let mut answer = 0u32;
    let mut size = u32::try_from(size_of::<u32>())
        .map_err(|error| RecordError::Other(io::Error::other(error)))?;
    // SAFETY: the names are NUL-terminated literals; `answer` is live, writable and `size` bytes
    // long for the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY,
            None,
            Some((&raw mut answer).cast()),
            Some(&raw mut size),
        )
    };
    Ok(present(read)?.then_some(answer))
}

/// The `REG_QWORD` `value` of `HKLM\<subkey>` (64-bit view); `None` when missing.
pub(crate) fn read_qword(subkey: PCWSTR, value: PCWSTR) -> Result<Option<u64>, RecordError> {
    let mut answer = 0u64;
    let mut size = u32::try_from(size_of::<u64>())
        .map_err(|error| RecordError::Other(io::Error::other(error)))?;
    // SAFETY: the names are NUL-terminated; `answer` is live, writable and `size` bytes long
    // for the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            RRF_RT_REG_QWORD | RRF_SUBKEY_WOW6464KEY,
            None,
            Some((&raw mut answer).cast()),
            Some(&raw mut size),
        )
    };
    Ok(present(read)?.then_some(answer))
}

/// What a `RegGetValueW` result means: `Ok(true)` read, `Ok(false)` missing. Every code but
/// these five is [`RecordError::Other`] (for example `ERROR_MORE_DATA` when the value grew
/// between the two reads, or `ERROR_NOT_ENOUGH_MEMORY`).
fn present(code: WIN32_ERROR) -> Result<bool, RecordError> {
    match code {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        ERROR_ACCESS_DENIED => Err(RecordError::Denied),
        ERROR_UNSUPPORTED_TYPE => Err(RecordError::WrongType),
        other => Err(RecordError::Other(io::Error::from_raw_os_error(
            i32::try_from(other.0).unwrap_or(i32::MAX),
        ))),
    }
}

/// This thread's user (`TokenUser`), in the canonical form `ConvertSidToStringSidW` writes: the
/// user of its impersonation token when it has one (set-up acting as the owner, S15 `PR4g`), so
/// the sandbox folder and its files are made for that user; else the process's user.
pub(crate) fn current_user_sid() -> io::Result<Sid> {
    let token = current_token()?;
    let text = user_of(token);
    // SAFETY: the handle came from `current_token` and is closed once, here. A failed close
    // leaves nothing to do.
    drop(unsafe { CloseHandle(token) });
    Sid::parse(&text?).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// The groups of the token [`current_user_sid`] reads that count in an access check, enabled
/// or deny-only (`TokenGroups`), each SID as bytes: what a deny can name to refuse this user.
pub(crate) fn current_token_groups() -> io::Result<Vec<Vec<u8>>> {
    /// `SE_GROUP_ENABLED` and `SE_GROUP_USE_FOR_DENY_ONLY`.
    const COUNTED: u32 = 0x4 | 0x10;
    let token = current_token()?;
    let read = token_info(token, TokenGroups, size_of::<TOKEN_GROUPS>());
    // SAFETY: the handle came from `current_token` and is closed once, here.
    drop(unsafe { CloseHandle(token) });
    let buffer = read?;
    // SAFETY: GetTokenInformation wrote a TOKEN_GROUPS at the start of `buffer`, aligned for
    // it, whose `GroupCount` entries and their SIDs lie in `buffer`, which outlives them.
    let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
    let count = usize::try_from(groups.GroupCount).map_err(io::Error::other)?;
    // SAFETY: as above, `Groups` holds `GroupCount` entries.
    let entries = unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), count) };
    let counted = entries.iter().filter(|g| g.Attributes & COUNTED != 0);
    counted
        .map(|g| {
            // SAFETY: each entry's SID is valid and lies in `buffer`, `GetLengthSid` bytes long.
            let len = usize::try_from(unsafe { GetLengthSid(g.Sid) }).map_err(io::Error::other)?;
            // SAFETY: as above.
            Ok(unsafe { std::slice::from_raw_parts(g.Sid.0.cast::<u8>(), len) }.to_vec())
        })
        .collect()
}

/// This thread's impersonation token when it has one, else the process's; the caller closes it.
fn current_token() -> io::Result<HANDLE> {
    let mut token = HANDLE::default();
    // SAFETY: GetCurrentThread returns a pseudo-handle that needs no close; `token` is a live
    // out-pointer for the call. Opened as self: the process's identity checks the open.
    match unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &raw mut token) } {
        Ok(()) => {}
        Err(error) if error.code() == ERROR_NO_TOKEN.to_hresult() => {
            // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `token`
            // is a live out-pointer for the call.
            unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(token)
}

/// `class` of `token`, in a buffer of at least `least` bytes aligned for pointers.
fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS, least: usize) -> io::Result<Vec<u64>> {
    let mut needed = 0u32;
    // SAFETY: no buffer and a zero length ask only for the size, written to `needed`.
    match unsafe { GetTokenInformation(token, class, None, 0, &raw mut needed) } {
        Err(error) if error.code() == HRESULT::from_win32(ERROR_INSUFFICIENT_BUFFER.0) => {}
        Err(error) => return Err(error.into()),
        Ok(()) => return Err(io::Error::other("the token gave no size")),
    }
    let bytes = usize::try_from(needed)
        .map_err(io::Error::other)?
        .max(least);
    let mut buffer = vec![0u64; bytes.div_ceil(size_of::<u64>())];
    let len = u32::try_from(size_of_val(buffer.as_slice())).map_err(io::Error::other)?;
    // SAFETY: `buffer` is live, writable and `len` bytes long for the call.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut needed,
        )
    }?;
    Ok(buffer)
}

/// The SID string of `token`'s user.
pub(crate) fn user_of(token: HANDLE) -> io::Result<String> {
    // u64 words: TOKEN_USER holds a pointer, so the buffer is aligned for it.
    let buffer = token_info(token, TokenUser, size_of::<TOKEN_USER>())?;
    // SAFETY: GetTokenInformation wrote a TOKEN_USER at the start of `buffer`, which is at least
    // that long and aligned for it; its SID points into `buffer`, which outlives `user`.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    // SAFETY: the SID is valid; `text` is a live out-pointer, freed with LocalFree below.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &raw mut text) }?;
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let sid = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    sid
}
