//! Tokens: this process's user, the facts `crate::token_check` judges, and the privileges a
//! `LowBox` token drops before the child gets it.
#![expect(
    unsafe_code,
    reason = "FFI to the token calls, GetSecurityInfo and the SID conversion"
)]

use std::ffi::c_void;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use ::windows::Win32::Foundation::{
    ERROR_BAD_LENGTH, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS, GetLastError, HANDLE, HLOCAL, LUID,
    LocalFree,
};
use ::windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_KERNEL_OBJECT,
};
use ::windows::Win32::Security::{
    AdjustTokenPrivileges, DACL_SECURITY_INFORMATION, GetTokenInformation,
    LABEL_SECURITY_INFORMATION, LUID_AND_ATTRIBUTES, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SE_PRIVILEGE_REMOVED, SID_AND_ATTRIBUTES,
    TOKEN_APPCONTAINER_INFORMATION, TOKEN_ELEVATION, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS,
    TOKEN_MANDATORY_LABEL, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER, TokenAppContainerSid,
    TokenCapabilities, TokenElevation, TokenGroups, TokenIntegrityLevel, TokenIsAppContainer,
    TokenPrivileges, TokenUser,
};
use ::windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use ::windows::core::PWSTR;

use super::win32;
use crate::error::Error;
use crate::package::Sid;
use crate::token_check::{CHANGE_NOTIFY, Facts};

/// `SE_GROUP_ENABLED`.
const GROUP_ENABLED: u32 = 0x4;
/// The first token information buffer: room for every variable-size class this module asks for
/// (a fixed-size class answers with its exact size, and `query` retries with it).
const FIRST_BYTES: u32 = 1024;
/// The largest token information buffer tried (64 KiB).
const MAX_BYTES: u32 = 0x1_0000;

/// This process's user.
pub(super) fn current_user() -> Result<Sid, Error> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo handle of this process; `token` gets a new handle, owned below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }
        .map_err(|e| win32("OpenProcessToken", &e))?;
    // SAFETY: OpenProcessToken succeeded, so `token` is open and ours alone.
    let token = unsafe { OwnedHandle::from_raw_handle(token.0) };
    let buffer = query(&token, TokenUser)?;
    // SAFETY: the call succeeded, so the buffer starts with a TOKEN_USER (8-byte aligned)
    // whose SID points into the same buffer, which is still alive.
    let sid = unsafe { buffer.as_ptr().cast::<TOKEN_USER>().read() }
        .User
        .Sid;
    // SAFETY: as above, `sid` points at a valid SID inside `buffer`.
    let text = unsafe { sid_text(sid) }?;
    Sid::parse(&text)
}

/// The `S-1-…` text of the owner of the object `handle` (any kernel object, the pipe folder's
/// file object included), else the Win32 error.
pub(super) fn owner_of(handle: HANDLE) -> Result<String, u32> {
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `handle` is a handle of this process (an invalid one fails the call); `owner`
    // points into `descriptor`, a LocalAlloc'd block freed once below.
    let code = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            None,
            None,
            Some(&raw mut descriptor),
        )
    };
    if code != ERROR_SUCCESS {
        return Err(code.0);
    }
    // SAFETY: the call succeeded, so `owner` points at a valid SID in `descriptor`.
    let text = unsafe { sid_text(owner) }.map_err(|_unconvertible| u32::MAX);
    // SAFETY: `descriptor` came from GetSecurityInfo (LocalAlloc) and is freed once, here; a
    // failure to free leaks a few bytes, nothing to undo.
    let _leak_on_failure: HLOCAL =
        unsafe { LocalFree(Some(HLOCAL(descriptor.0.cast::<c_void>()))) };
    text
}

/// The DACL (`D:…`) and mandatory label (`S:…`) of the object `handle` (opened with
/// `READ_CONTROL`) as SDDL, else the Win32 error.
pub(super) fn dacl_and_label_of(handle: HANDLE) -> Result<(String, String), u32> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `handle` is a handle of this process (an invalid one fails the call);
    // `descriptor` gets a LocalAlloc'd block freed once below.
    let code = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            Some(&raw mut descriptor),
        )
    };
    if code != ERROR_SUCCESS {
        return Err(code.0);
    }
    // SAFETY: the call succeeded, so `descriptor` is a valid descriptor holding both parts.
    let text = unsafe { super::nt::dacl_and_label(descriptor) };
    // SAFETY: `descriptor` came from GetSecurityInfo (LocalAlloc) and is freed once, here.
    let _leak_on_failure: HLOCAL =
        unsafe { LocalFree(Some(HLOCAL(descriptor.0.cast::<c_void>()))) };
    text
}

/// The facts `crate::token_check` reads from `token` (opened with `TOKEN_QUERY`).
pub(super) fn facts(token: &OwnedHandle) -> Result<Facts, Error> {
    let elevation = query(token, TokenElevation)?;
    // SAFETY: the call succeeded, so the buffer starts with a TOKEN_ELEVATION.
    let elevated = unsafe { elevation.as_ptr().cast::<TOKEN_ELEVATION>().read() }.TokenIsElevated;
    let boxed = query(token, TokenIsAppContainer)?;
    // SAFETY: the call succeeded, so the buffer starts with a DWORD.
    let app_container = unsafe { boxed.as_ptr().cast::<u32>().read() } != 0;
    let package = if app_container {
        let info = query(token, TokenAppContainerSid)?;
        // SAFETY: the call succeeded, so the buffer starts with a
        // TOKEN_APPCONTAINER_INFORMATION whose SID points into the same live buffer.
        let sid = unsafe {
            info.as_ptr()
                .cast::<TOKEN_APPCONTAINER_INFORMATION>()
                .read()
        }
        .TokenAppContainer;
        // SAFETY: as above.
        Some(unsafe { sid_text(sid) }?)
    } else {
        None
    };
    let label = query(token, TokenIntegrityLevel)?;
    // SAFETY: the call succeeded, so the buffer starts with a TOKEN_MANDATORY_LABEL whose SID
    // points into the same live buffer.
    let label_sid = unsafe { label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>().read() }
        .Label
        .Sid;
    // SAFETY: as above.
    let label_text = unsafe { sid_text(label_sid) }?;
    let integrity = label_text
        .rsplit('-')
        .next()
        .and_then(|rid| rid.parse::<u32>().ok())
        .ok_or(Error::SidText)?;
    let capabilities = groups(&query(token, TokenCapabilities)?)?.len();
    let enabled_groups = groups(&query(token, TokenGroups)?)?
        .into_iter()
        .filter(|(_, attributes)| attributes & GROUP_ENABLED != 0)
        .map(|(sid, _)| sid)
        .collect();
    let privileges = privileges(&query(token, TokenPrivileges)?)
        .iter()
        .map(|p| luid_value(p.Luid))
        .collect();
    Ok(Facts {
        elevated: elevated != 0,
        app_container,
        package,
        integrity,
        capabilities,
        privileges,
        enabled_groups,
    })
}

/// Removes every privilege of `token` (opened with `TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY`) but
/// `SeChangeNotifyPrivilege`, for good: a removed privilege cannot be enabled again.
pub(super) fn drop_privileges(token: &OwnedHandle) -> Result<(), Error> {
    let held = privileges(&query(token, TokenPrivileges)?);
    let removed: Vec<LUID_AND_ATTRIBUTES> = held
        .iter()
        .filter(|p| luid_value(p.Luid) != CHANGE_NOTIFY)
        .map(|p| LUID_AND_ATTRIBUTES {
            Luid: p.Luid,
            Attributes: SE_PRIVILEGE_REMOVED,
        })
        .collect();
    if removed.is_empty() {
        return Ok(());
    }
    // TOKEN_PRIVILEGES with `removed.len()` entries: the count, then the entries, in a u32
    // buffer (the struct's alignment is 4).
    let entry_words = size_of::<LUID_AND_ATTRIBUTES>() / 4;
    let mut buffer = vec![0_u32; 1 + entry_words * removed.len()];
    buffer[0] = u32::try_from(removed.len()).unwrap_or(u32::MAX);
    let first = (&raw mut buffer[1]).cast::<LUID_AND_ATTRIBUTES>();
    for (i, entry) in removed.iter().enumerate() {
        // SAFETY: entry `i` lies inside `buffer`, which holds `removed.len()` entries after the
        // count, at 4-byte alignment; write_unaligned needs no more.
        unsafe { first.add(i).write_unaligned(*entry) };
    }
    // SAFETY: `token` is open with TOKEN_ADJUST_PRIVILEGES; `buffer` is a TOKEN_PRIVILEGES of
    // the count it says, alive for the call; no previous state is asked for.
    unsafe {
        AdjustTokenPrivileges(
            HANDLE(token.as_raw_handle()),
            false,
            Some(buffer.as_ptr().cast::<TOKEN_PRIVILEGES>()),
            0,
            None,
            None,
        )
    }
    .map_err(|e| win32("AdjustTokenPrivileges", &e))?;
    // It succeeds with ERROR_NOT_ALL_ASSIGNED (1300) when some were not changed.
    // SAFETY: reads this thread's last error only.
    let last = unsafe { GetLastError() };
    if last != ERROR_SUCCESS {
        return Err(Error::Win32 {
            call: "AdjustTokenPrivileges (not every privilege removed)",
            code: last.0,
        });
    }
    Ok(())
}

/// `token`'s information of `class`, in an 8-byte aligned buffer: 1 KiB first, then the size
/// the call asks for, up to 64 KiB.
fn query(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<u64>, Error> {
    let mut bytes = FIRST_BYTES;
    loop {
        let mut buffer = vec![0_u64; (bytes as usize).div_ceil(8)];
        let mut needed = 0_u32;
        // SAFETY: `buffer` is writable and 8-byte aligned for `bytes` bytes, and outlives the
        // call; `needed` gets the size the information takes.
        let got = unsafe {
            GetTokenInformation(
                HANDLE(token.as_raw_handle()),
                class,
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                bytes,
                &raw mut needed,
            )
        };
        let error = match got {
            Ok(()) => return Ok(buffer),
            Err(e) => e,
        };
        let short = [ERROR_INSUFFICIENT_BUFFER, ERROR_BAD_LENGTH]
            .iter()
            .any(|c| error.code() == c.to_hresult());
        // A fixed-size class (TokenElevation) wants exactly its size and answers BAD_LENGTH with
        // a smaller `needed` (4 for 1024 on the VM), so retry with `needed` either way; the
        // same size again would loop, so that, nothing, or too much fails.
        if !short || needed == 0 || needed == bytes || needed > MAX_BYTES {
            return Err(win32("GetTokenInformation", &error));
        }
        bytes = needed;
    }
}

/// The SIDs and attributes of a `TOKEN_GROUPS` in `buffer`, as `query` returned it.
fn groups(buffer: &[u64]) -> Result<Vec<(String, u32)>, Error> {
    let base = buffer.as_ptr().cast::<TOKEN_GROUPS>();
    // SAFETY: the query succeeded, so the buffer starts with a TOKEN_GROUPS.
    let count = unsafe { (*base).GroupCount } as usize;
    // SAFETY: a place computation inside the struct, no read.
    let first = unsafe { (&raw const (*base).Groups) }.cast::<SID_AND_ATTRIBUTES>();
    let end = buffer.as_ptr_range().end.cast::<u8>();
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let entry = first.wrapping_add(i);
        if entry.wrapping_add(1).cast::<u8>() > end {
            // Never seen: the kernel sizes the buffer for every entry. Read nothing outside it.
            return Err(Error::SidText);
        }
        // SAFETY: the entry lies inside `buffer` (checked above), aligned as the struct is.
        let group = unsafe { entry.read() };
        // SAFETY: the kernel's SID pointers point into the same live buffer.
        out.push((unsafe { sid_text(group.Sid) }?, group.Attributes));
    }
    Ok(out)
}

/// The entries of a `TOKEN_PRIVILEGES` in `buffer`, as `query` returned it.
fn privileges(buffer: &[u64]) -> Vec<LUID_AND_ATTRIBUTES> {
    let base = buffer.as_ptr().cast::<TOKEN_PRIVILEGES>();
    // SAFETY: the query succeeded, so the buffer starts with a TOKEN_PRIVILEGES.
    let count = unsafe { (*base).PrivilegeCount } as usize;
    // SAFETY: a place computation inside the struct, no read.
    let first = unsafe { (&raw const (*base).Privileges) }.cast::<LUID_AND_ATTRIBUTES>();
    let end = buffer.as_ptr_range().end.cast::<u8>();
    (0..count)
        .map(|i| first.wrapping_add(i))
        .take_while(|entry| entry.wrapping_add(1).cast::<u8>() <= end)
        // SAFETY: each entry lies inside `buffer` (the take_while above), 4-byte aligned.
        .map(|entry| unsafe { entry.read() })
        .collect()
}

/// A LUID as one number.
fn luid_value(luid: LUID) -> u64 {
    (u64::from(luid.HighPart.cast_unsigned()) << 32) | u64::from(luid.LowPart)
}

/// The `S-1-…` text of `sid`.
///
/// # Safety
///
/// `sid` points at a valid SID for the call.
unsafe fn sid_text(sid: PSID) -> Result<String, Error> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` is valid (the caller's contract); `text` gets a LocalAlloc'd string, freed
    // below.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }
        .map_err(|e| win32("ConvertSidToStringSidW", &e))?;
    // SAFETY: the call succeeded, so `text` is a NUL-terminated string.
    let string = unsafe { text.to_string() };
    // SAFETY: `text` came from ConvertSidToStringSidW (LocalAlloc) and is freed once, here; a
    // failure to free leaks a few bytes, nothing to undo.
    let _leak_on_failure: HLOCAL = unsafe { LocalFree(Some(HLOCAL(text.0.cast::<c_void>()))) };
    string.map_err(|_not_utf16| Error::SidText)
}
