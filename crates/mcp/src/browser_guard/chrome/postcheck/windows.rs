//! Windows: Chrome's children in the guard's job (`postcheck.rs`). The processes are every
//! process in the job ([`Job::process_ids`]) but Chrome: the job holds only what the guard
//! started, so a renderer or GPU process in it is judged whoever started it, and no system-wide
//! snapshot is taken at each look. A process's start time is its creation time
//! (`GetProcessTimes`); its arguments are its command line (`NtQueryInformationProcess`,
//! `ProcessCommandLineInformation`, which needs only `PROCESS_QUERY_LIMITED_INFORMATION`); and
//! its sandbox is read from its primary token and Chrome's (`OpenProcessToken` with
//! `TOKEN_QUERY`): the integrity level and `IsTokenRestricted`, compared in `judge.rs`. Every
//! failed call reads as unknown, which the check refuses. Each call has one wrapper here, with
//! its `unsafe` block and why it is sound. Compiled for Windows, never run there yet (decision
//! record 0029, "Not verified").
#![expect(
    unsafe_code,
    reason = "FFI to the process and token APIs and NtQueryInformationProcess, for the check \
              of Chrome's sandbox"
)]

use std::collections::BTreeMap;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessCommandLineInformation};
use windows::Win32::Foundation::{HANDLE, UNICODE_STRING};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, IsTokenRestricted,
    IsValidSid, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenIntegrityLevel,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::super::super::job::Job;
use super::super::super::parent::windows::started;
use super::Source;
use super::crashed::Exits;
use super::judge::{self, Token};

/// The longest command line read, in bytes; Windows' own limit is 32 767 UTF-16 units.
const MAX_COMMAND_LINE: usize = 1 << 17;
/// `STILL_ACTIVE`: the exit code of a process that runs.
const STILL_ACTIVE: u32 = 259;
/// The most a token's integrity label takes, in bytes: a SID has at most 15 sub-authorities.
const MAX_LABEL: usize = 256;

/// The guard's job, read afresh at each call.
pub(super) struct InJob<'a> {
    pub(super) job: &'a Job,
}

impl Source for InJob<'_> {
    fn below(&mut self, chrome: u32) -> Option<Vec<u32>> {
        let mut in_job = self.job.process_ids()?;
        in_job.retain(|&pid| pid != chrome);
        Some(in_job)
    }

    fn stamp(&mut self, pid: u32) -> Option<u64> {
        started(HANDLE(open(pid)?.as_raw_handle()))
    }

    fn args(&mut self, pid: u32) -> Option<Vec<u8>> {
        command_line(&open(pid)?).map(String::into_bytes)
    }

    fn sandboxed(&mut self, chrome: u32, pid: u32) -> Result<(), &'static str> {
        judge::windows(
            open(chrome).as_ref().and_then(token),
            open(pid).as_ref().and_then(token),
        )
    }
}

/// The children of Chrome the watch during the wait for its endpoint holds open
/// (`crashed.rs`), so their exit codes stay readable after they exit.
#[derive(Default)]
pub(super) struct Held {
    handles: BTreeMap<u32, OwnedHandle>,
}

impl Exits for Held {
    fn hold(&mut self, pid: u32) {
        if let Some(process) = open(pid) {
            self.handles.insert(pid, process);
        }
    }

    fn exit_code(&mut self, pid: u32) -> Option<u32> {
        let process = self.handles.get(&pid)?;
        let mut code = 0_u32;
        // SAFETY: `process` is a live process handle with PROCESS_QUERY_LIMITED_INFORMATION,
        // which GetExitCodeProcess needs, and `code` a live u32.
        unsafe { GetExitCodeProcess(HANDLE(process.as_raw_handle()), &raw mut code) }.ok()?;
        (code != STILL_ACTIVE).then_some(code)
    }
}

/// Process `pid`, open for limited queries.
fn open(pid: u32) -> Option<OwnedHandle> {
    // SAFETY: opening a process by id takes no pointer; the handle is owned below.
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    // SAFETY: `raw` is the open handle OpenProcess just returned, which nothing else owns; the
    // `OwnedHandle` closes it once, on drop.
    Some(unsafe { OwnedHandle::from_raw_handle(raw.0) })
}

/// `process`'s command line. Windows writes a `UNICODE_STRING` into the buffer with the text
/// after it; the text is read from the buffer by offset, and only when the string's pointer and
/// length lie inside what was written.
fn command_line(process: &OwnedHandle) -> Option<String> {
    let handle = HANDLE(process.as_raw_handle());
    let mut needed = 0_u32;
    // SAFETY: a null buffer of 0 bytes asks only for the size, written to `needed`, a live u32.
    // The answer is a length-mismatch status; a success here (no command line) leaves
    // `needed` at 0, which is refused below.
    let asked = unsafe {
        NtQueryInformationProcess(
            handle,
            ProcessCommandLineInformation,
            std::ptr::null_mut(),
            0,
            &raw mut needed,
        )
    };
    let size = usize::try_from(needed).ok()?;
    if asked.is_ok() || size < size_of::<UNICODE_STRING>() || size > MAX_COMMAND_LINE {
        return None;
    }
    // Whole u64 words, so the UNICODE_STRING at the start is aligned.
    let mut words = vec![0_u64; size.div_ceil(8)];
    let room = words.len() * 8;
    // SAFETY: `words` is a live, zeroed, 8-byte-aligned buffer of `room` bytes, which the call
    // writes no further; `needed` is a live u32.
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            ProcessCommandLineInformation,
            words.as_mut_ptr().cast(),
            u32::try_from(room).ok()?,
            &raw mut needed,
        )
    };
    if status.is_err() {
        return None;
    }
    let base = words.as_ptr().addr();
    let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_ne_bytes()).collect();
    let length = usize::from(u16::from_ne_bytes(bytes.get(..2)?.try_into().ok()?));
    let at = std::mem::offset_of!(UNICODE_STRING, Buffer);
    let pointer = usize::from_ne_bytes(bytes.get(at..at + size_of::<usize>())?.try_into().ok()?);
    let start = pointer.checked_sub(base)?;
    if start < size_of::<UNICODE_STRING>() || start % 2 != 0 || length % 2 != 0 {
        return None;
    }
    let text = bytes.get(start..start.checked_add(length)?)?;
    let units: Vec<u16> = text
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&unit| u16::from_ne_bytes(unit))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// What `process`'s primary token says; `None` when any of it cannot be read.
fn token(process: &OwnedHandle) -> Option<Token> {
    let mut raw = HANDLE::default();
    // SAFETY: `process` is a live process handle with query rights and `raw` a live HANDLE the
    // call writes the token's handle to, which is owned below.
    unsafe { OpenProcessToken(HANDLE(process.as_raw_handle()), TOKEN_QUERY, &raw mut raw) }.ok()?;
    // SAFETY: `raw` is the open token handle OpenProcessToken just returned, which nothing else
    // owns; the `OwnedHandle` closes it once, on drop.
    let token = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let handle = HANDLE(token.as_raw_handle());
    // SAFETY: `handle` is the live token handle `token` owns, opened with TOKEN_QUERY.
    let restricted = unsafe { IsTokenRestricted(handle) }.is_ok();
    Some(Token {
        integrity: integrity(handle)?,
        restricted,
    })
}

/// The RID of token `handle`'s integrity level: the last sub-authority of its label's SID.
fn integrity(handle: HANDLE) -> Option<u32> {
    let mut needed = 0_u32;
    // SAFETY: no buffer and a length of 0 ask only for the size, written to `needed`, a live
    // u32; the call fails as it must, and `needed` says how much to make room for.
    drop(unsafe { GetTokenInformation(handle, TokenIntegrityLevel, None, 0, &raw mut needed) });
    let size = usize::try_from(needed).ok()?;
    if size < size_of::<TOKEN_MANDATORY_LABEL>() || size > MAX_LABEL {
        return None;
    }
    let mut words = vec![0_u64; size.div_ceil(8)];
    let room = words.len() * 8;
    // SAFETY: `words` is a live, zeroed, 8-byte-aligned buffer of `room` bytes, which the call
    // fills with a TOKEN_MANDATORY_LABEL and the SID it points to, writing no further.
    unsafe {
        GetTokenInformation(
            handle,
            TokenIntegrityLevel,
            Some(words.as_mut_ptr().cast()),
            u32::try_from(room).ok()?,
            &raw mut needed,
        )
    }
    .ok()?;
    // SAFETY: `words` is aligned for and larger than a TOKEN_MANDATORY_LABEL, which the call
    // wrote at its start.
    let label = unsafe { words.as_ptr().cast::<TOKEN_MANDATORY_LABEL>().read() };
    let sid = label.Label.Sid;
    let base = words.as_ptr().addr();
    let at = sid.0.addr().checked_sub(base)?;
    if at < size_of::<TOKEN_MANDATORY_LABEL>() || at >= room {
        return None;
    }
    // SAFETY: `sid` points inside `words` (checked above), live until this function returns;
    // IsValidSid reads only the SID's header and the sub-authorities it counts, and refuses a
    // malformed one.
    if !unsafe { IsValidSid(sid) }.as_bool() {
        return None;
    }
    // SAFETY: `sid` is a valid SID inside `words` (above); the call returns a pointer to its
    // count byte, inside the SID.
    let count = unsafe { *GetSidSubAuthorityCount(sid) };
    let last = u32::from(count.checked_sub(1)?);
    // SAFETY: `sid` is a valid SID with `count` sub-authorities and `last` is below `count`, so
    // the pointer returned is to one of them, inside the SID.
    Some(unsafe { *GetSidSubAuthority(sid, last) })
}
