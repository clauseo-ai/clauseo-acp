//! Stopping the processes that run as a pool user (S15 `PR4f`, D-B13), before a profile
//! delete: removal calls it for the whole pool, and `delete_profile` for its one SID.
//!
//! Each process is opened once, by pid, and its token user read through that same handle; the
//! termination goes through that handle too, so a pid reused between the list and the
//! termination can't redirect it. Which processes to stop is [`to_stop`] (pure, tested on every
//! OS): a token user equal to a pool SID, nothing else.
#![expect(
    unsafe_code,
    reason = "FFI to the process list, process tokens and TerminateProcess"
)]

use windows::Win32::Foundation::{HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_DEBUG_NAME,
    SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::ProcessStatus::EnumProcesses;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
    WaitForSingleObject,
};
use windows::core::PCWSTR;

use super::ffi::{code_of, os, token_user};
use super::machine::MachineError;
use super::strays::{Seen, to_stop};
use crate::windows::acl::object::Handle;
use crate::windows::sids::Sid;

/// The exit code a stopped process gets.
const STOPPED_EXIT: u32 = 1;
/// How long each stopped process is waited for, in milliseconds.
const EXIT_WAIT_MS: u32 = 5_000;
/// `ERROR_ACCESS_DENIED`, for a process whose user was read but which couldn't be opened to
/// be stopped.
const ACCESS_DENIED: u32 = 5;

/// What [`stop_pool_processes`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stopped {
    /// The pids stopped.
    pub stopped: Vec<u32>,
    /// The pids that run as a pool user but couldn't be stopped, each with its error code.
    pub failed: Vec<(u32, u32)>,
}

/// Stops every process whose token user is one of `pool`, waiting up to five seconds for each
/// to exit. Enables `SeDebugPrivilege` first where the token holds it (an administrator's
/// does), since a pool user's process grants nothing to Administrators. A process that can't
/// be opened at all is passed over: its user is unknown, so it is never stopped.
pub fn stop_pool_processes(pool: &[Sid]) -> Result<Stopped, MachineError> {
    let mut done = Stopped::default();
    if pool.is_empty() {
        return Ok(done);
    }
    enable_debug();
    let listed = look()?;
    let seen: Vec<Seen> = listed.iter().map(|(seen, _)| seen.clone()).collect();
    // SAFETY: GetCurrentProcessId only reads this process's id.
    let own = unsafe { GetCurrentProcessId() };
    for pid in to_stop(&seen, pool, own) {
        let handle = listed
            .iter()
            .find(|(seen, _)| seen.pid == pid)
            .and_then(|(_, handle)| handle.as_ref());
        let Some(handle) = handle else {
            done.failed.push((pid, ACCESS_DENIED));
            continue;
        };
        // SAFETY: the handle is open with PROCESS_TERMINATE on the process whose user was read.
        match unsafe { TerminateProcess(handle.0, STOPPED_EXIT) } {
            Ok(()) => {
                // SAFETY: the handle is open with PROCESS_SYNCHRONIZE; the wait is bounded.
                unsafe { WaitForSingleObject(handle.0, EXIT_WAIT_MS) };
                done.stopped.push(pid);
            }
            Err(error) => done.failed.push((pid, code_of(&error))),
        }
    }
    Ok(done)
}

/// Every running process with its token user, for the tests.
#[cfg(test)]
pub(super) fn running() -> Result<Vec<Seen>, MachineError> {
    Ok(look()?.into_iter().map(|(seen, _)| seen).collect())
}

/// Every running process: what was seen, and a handle that can stop it when one could be
/// opened with `PROCESS_TERMINATE`.
fn look() -> Result<Vec<(Seen, Option<Handle>)>, MachineError> {
    let mut listed = Vec::new();
    for pid in pids()? {
        let full = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
        // SAFETY: OpenProcess takes plain values; the handle is owned by `Handle` from here.
        let opened = unsafe { OpenProcess(full, false, pid) }.map(|handle| (Handle(handle), true));
        let opened = opened.or_else(|_no_terminate| {
            // SAFETY: as above, with the query right alone.
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                .map(|handle| (Handle(handle), false))
        });
        let Ok((handle, can_stop)) = opened else {
            listed.push((Seen { pid, user: None }, None));
            continue;
        };
        let user = user_of(&handle);
        listed.push((Seen { pid, user }, can_stop.then_some(handle)));
    }
    Ok(listed)
}

/// The user of the process `process` is open on; `None` when its token can't be read.
fn user_of(process: &Handle) -> Option<Sid> {
    let mut token = HANDLE::default();
    // SAFETY: the process handle is open with PROCESS_QUERY_LIMITED_INFORMATION; `token` is a
    // live out-pointer, owned by `Handle` once written.
    unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &raw mut token) }.ok()?;
    let token = Handle(token);
    token_user(token.0).ok().flatten()
}

/// Every running process's pid (`EnumProcesses`), the buffer grown until the list fits.
fn pids() -> Result<Vec<u32>, MachineError> {
    let mut buffer = vec![0u32; 1024];
    loop {
        let size = u32::try_from(size_of_val(buffer.as_slice())).unwrap_or(u32::MAX);
        let mut needed = 0u32;
        // SAFETY: `buffer` is live and writable for `size` bytes; `needed` is a live out-pointer.
        unsafe { EnumProcesses(buffer.as_mut_ptr(), size, &raw mut needed) }
            .map_err(|error| os("EnumProcesses", &error))?;
        // A full buffer may have cut the list short: grow it and ask again.
        if needed < size {
            let count = usize::try_from(needed).unwrap_or(0) / size_of::<u32>();
            buffer.truncate(count);
            return Ok(buffer);
        }
        buffer.resize(buffer.len() * 2, 0);
    }
}

/// Enables `SeDebugPrivilege` in this process's token, where it holds the privilege. Without
/// it, the stop still reaches the processes whose DACL lets this token in.
fn enable_debug() {
    let mut token = HANDLE::default();
    let access = TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY;
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `token` is a live
    // out-pointer, owned by `Handle` once written.
    if unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut token) }.is_err() {
        return;
    }
    let token = Handle(token);
    let mut luid = LUID::default();
    // SAFETY: no system name (this one); the privilege name is a NUL-terminated constant;
    // `luid` is a live out-pointer.
    if unsafe { LookupPrivilegeValueW(PCWSTR::null(), SE_DEBUG_NAME, &raw mut luid) }.is_err() {
        return;
    }
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    let size = u32::try_from(size_of::<TOKEN_PRIVILEGES>()).unwrap_or(0);
    // SAFETY: the token is open with TOKEN_ADJUST_PRIVILEGES; `privileges` is live and `size`
    // bytes long; no previous state is asked for.
    let adjusted = unsafe {
        AdjustTokenPrivileges(
            token.0,
            false,
            Some(&raw const privileges),
            size,
            None,
            None,
        )
    };
    if let Err(error) = adjusted {
        tracing::debug!(code = error.code().0, "SeDebugPrivilege wasn't enabled");
    }
}
