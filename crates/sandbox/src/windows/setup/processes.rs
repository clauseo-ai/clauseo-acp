//! Stopping the processes that run as a pool user (S15 `PR4f`, D-B13), before a profile
//! delete: removal calls it for the whole pool, and `delete_profile` for its one SID.
//!
//! Each process is opened once, by pid, and its token user read through that same handle; the
//! termination goes through that handle too, so a pid reused between the list and the
//! termination can't redirect it. Which processes to stop is [`to_stop`] (pure, tested on every
//! OS): a token user equal to a pool SID, nothing else. Before anything, every SID asked for
//! must be a local account of this machine, not built in, and not this process's user.
#![expect(
    unsafe_code,
    reason = "FFI to the process list, process tokens and TerminateProcess"
)]

use windows::Win32::Foundation::{HANDLE, LUID, WAIT_OBJECT_0};
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
use super::preflight::{account_name, machine_domain};
use super::strays::{Seen, check_pool_sid, to_stop};
use crate::windows::acl::object::Handle;
use crate::windows::record::current_user_sid;
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
    /// The pids stopped and seen to exit.
    pub ended: Vec<u32>,
    /// The pids that run as a pool user but couldn't be stopped, each with its error code.
    pub failed: Vec<(u32, u32)>,
    /// The pids told to stop that hadn't exited when the wait ran out.
    pub timed_out: Vec<u32>,
}

/// Stops every process whose token user is one of `pool`, waiting up to five seconds for each
/// to exit. Refuses the whole call, stopping nothing, when any SID of `pool` fails
/// [`check_pool`] (`owner` is the user who asked for the set-up). Enables
/// `SeDebugPrivilege` (where the token holds it, as an administrator's does) only around the
/// list and the stops, since a pool user's process grants nothing to Administrators. A process
/// that can't be opened at all is passed over: its user is unknown, so it is never stopped.
pub fn stop_pool_processes(pool: &[Sid], owner: &Sid) -> Result<Stopped, MachineError> {
    let mut done = Stopped::default();
    if pool.is_empty() {
        return Ok(done);
    }
    check_pool(pool, owner)?;
    let debug = DebugPrivilege::enable();
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
                if unsafe { WaitForSingleObject(handle.0, EXIT_WAIT_MS) } == WAIT_OBJECT_0 {
                    done.ended.push(pid);
                } else {
                    done.timed_out.push(pid);
                }
            }
            Err(error) => done.failed.push((pid, code_of(&error))),
        }
    }
    drop(listed);
    drop(debug);
    Ok(done)
}

/// Refuses `pool` unless every SID passes [`check_pool_sid`] against this machine's account
/// domain, this process's user, `owner` (the user who asked for the set-up) and the name the
/// SID resolves to: defence in depth, so no caller can stop another user's processes or delete
/// another user's profile.
pub(super) fn check_pool(pool: &[Sid], owner: &Sid) -> Result<(), MachineError> {
    let machine = machine_domain()?;
    let own = current_user_sid().map_err(|error| MachineError::Os {
        call: "GetTokenInformation",
        code: error
            .raw_os_error()
            .and_then(|code| u32::try_from(code).ok())
            .unwrap_or(u32::MAX),
    })?;
    for sid in pool {
        let name = account_name(sid)?;
        check_pool_sid(sid, machine, &own, owner, name.as_deref()).map_err(|why| {
            MachineError::Other(format!("{} isn't a pool user: {why}", sid.as_str()))
        })?;
    }
    Ok(())
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

/// `SeDebugPrivilege` enabled in this process's token for as long as this guard lives; on drop
/// the token gets back the state it had before (`PreviousState`), on every return path.
struct DebugPrivilege {
    /// The token, open for `TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY`.
    token: Handle,
    /// The privilege's state before the change; restored on drop.
    previous: TOKEN_PRIVILEGES,
}

impl DebugPrivilege {
    /// Enables the privilege where the token holds it; `None` when it couldn't (the stop then
    /// still reaches the processes whose DACL lets this token in).
    fn enable() -> Option<DebugPrivilege> {
        let mut token = HANDLE::default();
        let access = TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY;
        // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `token` is a
        // live out-pointer, owned by `Handle` once written.
        unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut token) }.ok()?;
        let token = Handle(token);
        let mut luid = LUID::default();
        // SAFETY: no system name (this one); the privilege name is a NUL-terminated constant;
        // `luid` is a live out-pointer.
        unsafe { LookupPrivilegeValueW(PCWSTR::null(), SE_DEBUG_NAME, &raw mut luid) }.ok()?;
        let wanted = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let mut previous = TOKEN_PRIVILEGES::default();
        let size = u32::try_from(size_of::<TOKEN_PRIVILEGES>()).unwrap_or(0);
        let mut returned = 0u32;
        // SAFETY: the token is open with TOKEN_ADJUST_PRIVILEGES; `wanted` and `previous` are
        // live and `size` bytes long (one privilege each); `returned` is a live out-pointer.
        let adjusted = unsafe {
            AdjustTokenPrivileges(
                token.0,
                false,
                Some(&raw const wanted),
                size,
                Some(&raw mut previous),
                Some(&raw mut returned),
            )
        };
        if let Err(error) = adjusted {
            tracing::debug!(code = error.code().0, "SeDebugPrivilege wasn't enabled");
            return None;
        }
        Some(DebugPrivilege { token, previous })
    }
}

impl Drop for DebugPrivilege {
    fn drop(&mut self) {
        // A privilege the token lacked changed nothing: `previous` then lists none.
        if self.previous.PrivilegeCount == 0 {
            return;
        }
        let size = u32::try_from(size_of::<TOKEN_PRIVILEGES>()).unwrap_or(0);
        // SAFETY: the token is still open with TOKEN_ADJUST_PRIVILEGES (it drops after this);
        // `previous` is live and `size` bytes long; no previous state is asked for.
        let restored = unsafe {
            AdjustTokenPrivileges(
                self.token.0,
                false,
                Some(&raw const self.previous),
                size,
                None,
                None,
            )
        };
        if let Err(error) = restored {
            tracing::warn!(
                code = error.code().0,
                "SeDebugPrivilege's state wasn't restored"
            );
        }
    }
}
