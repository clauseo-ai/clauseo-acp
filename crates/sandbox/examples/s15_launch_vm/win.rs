//! The Win32 calls the harness and its probes make: the token's parts, `OpenProcess`, `CreateFileW`
//! on a pipe or a file, the pipe listing, an account's name, a file's owner, and a wait for a pid
//! to end. Each gives a plain value or the Win32 error code, never a panic.
#![expect(
    unsafe_code,
    reason = "FFI to the token, process, file, pipe and SID calls"
)]

use std::io;
use std::time::Duration;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree,
    WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSidToSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, LookupPrivilegeNameW, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SID_NAME_USE, TOKEN_ADJUST_DEFAULT, TOKEN_GROUPS,
    TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
    TokenGroups, TokenIntegrityLevel, TokenPrivileges, TokenRestrictedSids, TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, FindClose,
    FindFirstFileW, FindNextFileW, OPEN_EXISTING, WIN32_FIND_DATAW,
};
use windows::Win32::System::JobObjects::{
    JOB_OBJECT_UILIMIT_DESKTOP, JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_HANDLES, JOB_OBJECT_UILIMIT_READCLIPBOARD,
    JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JobObjectBasicUIRestrictions, QueryInformationJobObject,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_ACCESS_RIGHTS,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
    WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// The UI limits the runner's job must carry (S15 PR5): every one, `DESKTOP` included.
pub(crate) const UI_LIMITS: u32 = JOB_OBJECT_UILIMIT_READCLIPBOARD.0
    | JOB_OBJECT_UILIMIT_WRITECLIPBOARD.0
    | JOB_OBJECT_UILIMIT_GLOBALATOMS.0
    | JOB_OBJECT_UILIMIT_HANDLES.0
    | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS.0
    | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS.0
    | JOB_OBJECT_UILIMIT_EXITWINDOWS.0
    | JOB_OBJECT_UILIMIT_DESKTOP.0;

/// The UI limits of the job this process is in (`JobObjectBasicUIRestrictions` through
/// `QueryInformationJobObject(None, …)`).
pub(crate) fn job_ui_limits() -> io::Result<u32> {
    let mut limits = JOBOBJECT_BASIC_UI_RESTRICTIONS::default();
    let len = u32::try_from(size_of_val(&limits)).map_err(io::Error::other)?;
    // SAFETY: `limits` is a live, writable struct of `len` bytes.
    unsafe {
        QueryInformationJobObject(
            None,
            JobObjectBasicUIRestrictions,
            (&raw mut limits).cast(),
            len,
            None,
        )
    }
    .map_err(|e| os(&e))?;
    Ok(limits.UIRestrictionsClass.0)
}

/// `ERROR_INVALID_PARAMETER`: `OpenProcess` on a pid no process has.
const NO_SUCH_PROCESS: u32 = 87;

/// What a token holds, as text.
#[derive(Debug, Default)]
pub(crate) struct TokenFacts {
    /// The user's SID.
    pub(crate) user: String,
    /// The integrity level's SID.
    pub(crate) integrity: String,
    /// `TokenRestrictedSids`, in order.
    pub(crate) restricted: Vec<String>,
    /// `TokenGroups`: each SID and its attributes.
    pub(crate) groups: Vec<(String, u32)>,
    /// The privileges' names and attributes.
    pub(crate) privileges: Vec<(String, u32)>,
}

/// A handle closed on drop.
pub(crate) struct Owned(pub(crate) HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle is open and closed once, here.
        drop(unsafe { CloseHandle(self.0) });
    }
}

/// The Win32 code in `error`.
pub(crate) fn code_of(error: &windows::core::Error) -> u32 {
    error.code().0.cast_unsigned() & 0xFFFF
}

fn os(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(code_of(error)).unwrap_or(-1))
}

/// This process's token's facts.
pub(crate) fn own_token() -> io::Result<TokenFacts> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo-handle needs no closing; `token` is a live out-pointer.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }
        .map_err(|e| os(&e))?;
    token_facts(Owned(token).0)
}

/// The code `OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_DEFAULT)` gives on this
/// process's own token; 0 when it opened (closed again at once).
pub(crate) fn own_token_adjust_default_code() -> u32 {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo-handle needs no closing; `token` is a live out-pointer, closed by
    // `Owned`.
    match unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_DEFAULT, &raw mut token) } {
        Ok(()) => {
            drop(Owned(token));
            0
        }
        Err(error) => code_of(&error),
    }
}

/// What [`own_token`] reads, from `token` (a live handle with `TOKEN_QUERY`).
pub(crate) fn token_facts(token: HANDLE) -> io::Result<TokenFacts> {
    let token = Borrowed(token);
    let user = information(token.0, TokenUser)?;
    // SAFETY: GetTokenInformation wrote a TOKEN_USER at the start of the aligned buffer.
    let user = unsafe { &*user.as_ptr().cast::<TOKEN_USER>() };
    let label = information(token.0, TokenIntegrityLevel)?;
    // SAFETY: as above, a TOKEN_MANDATORY_LABEL.
    let label = unsafe { &*label.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    let restricted = groups(&information(token.0, TokenRestrictedSids)?)?;
    Ok(TokenFacts {
        user: sid_text(user.User.Sid)?,
        integrity: sid_text(label.Label.Sid)?,
        restricted: restricted.into_iter().map(|(sid, _)| sid).collect(),
        groups: groups(&information(token.0, TokenGroups)?)?,
        privileges: privileges(token.0)?,
    })
}

/// The user SID of `token` (a live handle with `TOKEN_QUERY`), as text.
pub(crate) fn token_user(token: HANDLE) -> io::Result<String> {
    let user = information(token, TokenUser)?;
    // SAFETY: GetTokenInformation wrote a TOKEN_USER at the start of the aligned buffer.
    let user = unsafe { &*user.as_ptr().cast::<TOKEN_USER>() };
    sid_text(user.User.Sid)
}

/// A token handle someone else closes.
struct Borrowed(HANDLE);

/// `class` of `token`, in u64 words so any of the structures is aligned in it.
fn information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
    let mut needed = 0u32;
    // SAFETY: no buffer and a zero length ask only for the size, written to `needed`.
    match unsafe { GetTokenInformation(token, class, None, 0, &raw mut needed) } {
        Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {}
        Err(error) => return Err(os(&error)),
        Ok(()) => return Ok(vec![0u64; 1]),
    }
    let words = usize::try_from(needed)
        .map_err(io::Error::other)?
        .div_ceil(8);
    let mut buffer = vec![0u64; words.max(1)];
    let len = u32::try_from(size_of_val(buffer.as_slice())).map_err(io::Error::other)?;
    // SAFETY: `buffer` is live, writable and `len` bytes long.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut needed,
        )
    }
    .map_err(|e| os(&e))?;
    Ok(buffer)
}

/// The groups of a `TOKEN_GROUPS` that `buffer` holds, as (SID, attributes), in order.
fn groups(buffer: &[u64]) -> io::Result<Vec<(String, u32)>> {
    // SAFETY: GetTokenInformation wrote a TOKEN_GROUPS at the start of `buffer`, aligned for
    // it; its SIDs point into `buffer`.
    let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
    let count = usize::try_from(groups.GroupCount).map_err(io::Error::other)?;
    // SAFETY: `Groups` is a variable-length array of `GroupCount` entries inside `buffer`.
    let entries = unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), count) };
    entries
        .iter()
        .map(|group| Ok((sid_text(group.Sid)?, group.Attributes)))
        .collect()
}

/// The names and attributes of `token`'s privileges, in order.
fn privileges(token: HANDLE) -> io::Result<Vec<(String, u32)>> {
    let buffer = information(token, TokenPrivileges)?;
    // SAFETY: GetTokenInformation wrote a TOKEN_PRIVILEGES at the start of `buffer`.
    let held = unsafe { &*buffer.as_ptr().cast::<TOKEN_PRIVILEGES>() };
    let count = usize::try_from(held.PrivilegeCount).map_err(io::Error::other)?;
    // SAFETY: `Privileges` is a variable-length array of `PrivilegeCount` entries in `buffer`.
    let entries = unsafe { std::slice::from_raw_parts(held.Privileges.as_ptr(), count) };
    let mut out = Vec::new();
    for entry in entries {
        let mut name = vec![0u16; 128];
        let mut len = u32::try_from(name.len()).map_err(io::Error::other)?;
        // SAFETY: `entry.Luid` lives for the call; `name` is writable for `len` characters.
        unsafe {
            LookupPrivilegeNameW(
                None,
                &raw const entry.Luid,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut len,
            )
        }
        .map_err(|e| os(&e))?;
        let end = usize::try_from(len).map_err(io::Error::other)?;
        let text = String::from_utf16_lossy(name.get(..end).unwrap_or_default());
        out.push((text, entry.Attributes.0));
    }
    Ok(out)
}

/// `sid` as text.
pub(crate) fn sid_text(sid: PSID) -> io::Result<String> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` points at a valid SID for the call; `text` is a live out-pointer.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.map_err(|e| os(&e))?;
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let read = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    read
}

/// The error code `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` gives for `pid`; 0 when it
/// opened.
pub(crate) fn open_process_code(pid: u32) -> u32 {
    // SAFETY: plain call; the handle, when one comes back, is closed by `Owned`.
    match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(handle) => {
            drop(Owned(handle));
            0
        }
        Err(error) => code_of(&error),
    }
}

/// The error code `CreateFileW(OPEN_EXISTING)` gives for `path` opened for reading or for
/// writing; 0 when it opened (it is closed again at once, nothing written).
pub(crate) fn open_file_code(path: &str, write: bool) -> u32 {
    let access = if write {
        GENERIC_WRITE.0
    } else {
        GENERIC_READ.0
    };
    // SAFETY: the name is a live HSTRING for the call; the handle is closed by `Owned`.
    let opened = unsafe {
        CreateFileW(
            &HSTRING::from(path),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    };
    match opened {
        Ok(handle) => {
            drop(Owned(handle));
            0
        }
        Err(error) => code_of(&error),
    }
}

/// The names under `\\.\pipe\` that start with `prefix`.
pub(crate) fn list_pipes(prefix: &str) -> io::Result<Vec<String>> {
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: the pattern is a live HSTRING; `data` is a live out-pointer.
    let find = unsafe { FindFirstFileW(&HSTRING::from(r"\\.\pipe\*"), &raw mut data) }
        .map_err(|e| os(&e))?;
    let mut names = Vec::new();
    loop {
        let end = data
            .cFileName
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(data.cFileName.len());
        let name = String::from_utf16_lossy(data.cFileName.get(..end).unwrap_or_default());
        if name.starts_with(prefix) {
            names.push(name);
        }
        // SAFETY: `find` is the open search; `data` is a live out-pointer.
        if unsafe { FindNextFileW(find, &raw mut data) }.is_err() {
            break;
        }
    }
    // SAFETY: the search is open and closed once, here.
    drop(unsafe { FindClose(find) });
    Ok(names)
}

/// The account name (without its domain) of the SID `sid`.
pub(crate) fn account_name(sid: &str) -> io::Result<String> {
    let mut parsed = PSID::default();
    // SAFETY: the text is a live HSTRING; `parsed` is a live out-pointer, freed below.
    unsafe { ConvertStringSidToSidW(&HSTRING::from(sid), &raw mut parsed) }.map_err(|e| os(&e))?;
    let mut name = vec![0u16; 256];
    let mut domain = vec![0u16; 256];
    let mut name_len = u32::try_from(name.len()).map_err(io::Error::other)?;
    let mut domain_len = u32::try_from(domain.len()).map_err(io::Error::other)?;
    let mut kind = SID_NAME_USE::default();
    // SAFETY: `parsed` is a valid SID; both buffers are writable for their lengths.
    let looked = unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            parsed,
            Some(PWSTR(name.as_mut_ptr())),
            &raw mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_len,
            &raw mut kind,
        )
    };
    // SAFETY: ConvertStringSidToSidW allocated the SID with LocalAlloc; freed once.
    unsafe { LocalFree(Some(HLOCAL(parsed.0))) };
    looked.map_err(|e| os(&e))?;
    let end = usize::try_from(name_len).map_err(io::Error::other)?;
    Ok(String::from_utf16_lossy(
        name.get(..end).unwrap_or_default(),
    ))
}

/// The owner SID of the file at `path`.
pub(crate) fn file_owner(path: &str) -> io::Result<String> {
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the name is a live HSTRING; both out-pointers are live; `owner` points into the
    // descriptor, freed after the owner is read.
    let code = unsafe {
        GetNamedSecurityInfoW(
            &HSTRING::from(path),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            None,
            None,
            &raw mut descriptor,
        )
    };
    if code.0 != 0 {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(code.0).unwrap_or(-1),
        ));
    }
    let text = sid_text(owner);
    // SAFETY: allocated by GetNamedSecurityInfoW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    text
}

/// Ends the process `pid` (one the harness started) with `TerminateProcess`; the Win32 code, 0
/// when it ended.
pub(crate) fn terminate(pid: u32) -> u32 {
    // SAFETY: plain call; the handle is closed by `Owned`.
    let handle = match unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) } {
        Ok(handle) => Owned(handle),
        Err(error) => return code_of(&error),
    };
    // SAFETY: the handle is open with PROCESS_TERMINATE.
    match unsafe { TerminateProcess(handle.0, 1) } {
        Ok(()) => 0,
        Err(error) => code_of(&error),
    }
}

/// Whether the process `pid` is gone, or ends within `timeout`: `OpenProcess(SYNCHRONIZE)` and a
/// wait. `Err` with the code when it can't be opened for another reason than being gone.
pub(crate) fn gone_within(pid: u32, timeout: Duration) -> Result<bool, u32> {
    let access: PROCESS_ACCESS_RIGHTS = PROCESS_SYNCHRONIZE;
    // SAFETY: plain call; the handle is closed by `Owned`.
    let handle = match unsafe { OpenProcess(access, false, pid) } {
        Ok(handle) => Owned(handle),
        Err(error) if code_of(&error) == NO_SUCH_PROCESS => return Ok(true),
        Err(error) => return Err(code_of(&error)),
    };
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the handle is open for SYNCHRONIZE.
    let woke = unsafe { WaitForSingleObject(handle.0, millis) };
    Ok(woke == WAIT_OBJECT_0)
}
