//! Windows only: the guard's process identity, the twin of the Unix pid and version
//! (`transport.rs`'s `Identity`): its pid and its creation time (a FILETIME, in 100 ns units),
//! which a later process that reuses the pid cannot have.
//!
//! The guard prints its own ([`own`]); the host passes it to the bridge as `--guard-pid` and
//! `--guard-pidversion`; the bridge checks each pipe it connects to against it
//! ([`server_is`]): the pipe's server process id (`GetNamedPipeServerProcessId`) must be the pid,
//! and that process, opened for `PROCESS_QUERY_LIMITED_INFORMATION`, must have that creation
//! time. That keeps another process of the browser's account (agent-browser or its daemon, which
//! hold the same owner rights on the pipe) from adding an instance and posing as the guard.
//!
//! The bridge runs as the agent's account (an `AppContainer` one soon), which by default may not
//! open the guard's process at all. So before it listens the guard grants that account, and the
//! agent's package when given, `PROCESS_QUERY_LIMITED_INFORMATION` on its own process and nothing
//! more ([`open_to`]): it adds two allow entries at most to its own process DACL.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 process and security APIs, for the guard's process identity"
)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{ERROR_SUCCESS, FILETIME, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    EXPLICIT_ACCESS_W, GRANT_ACCESS, GetSecurityInfo, NO_MULTIPLE_TRUSTEE, SE_KERNEL_OBJECT,
    SetEntriesInAclW, SetSecurityInfo, TRUSTEE_IS_SID, TRUSTEE_IS_UNKNOWN, TRUSTEE_W,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, NO_INHERITANCE, PSECURITY_DESCRIPTOR,
};
use windows::Win32::System::Pipes::GetNamedPipeServerProcessId;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::core::PWSTR;

use super::sid::Sid;
use crate::transport::Identity;

/// This process's identity: its pid and creation time.
pub(crate) fn own() -> Option<Identity> {
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing.
    let process = unsafe { GetCurrentProcess() };
    Some(Identity {
        pid: std::process::id(),
        version: created(process)?,
    })
}

/// The creation time of the process `process` (opened with query rights), in 100 ns units.
fn created(process: HANDLE) -> Option<u64> {
    let (mut creation, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: `process` is a live process handle with query rights; the four outputs are live.
    unsafe {
        GetProcessTimes(
            process,
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    }
    .ok()?;
    Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime))
}

/// The bridge's identity check: whether the server end of the pipe `pipe` (the bridge's own
/// handle) is held by the process `guard`. `false` on any failure.
pub(crate) fn server_is(pipe: &OwnedHandle, guard: Identity) -> bool {
    let mut pid = 0u32;
    // SAFETY: `pipe` is the open client end of a pipe; `pid` is live.
    if unsafe { GetNamedPipeServerProcessId(HANDLE(pipe.as_raw_handle()), &raw mut pid) }.is_err()
        || pid != guard.pid
    {
        return false;
    }
    // SAFETY: opening a process by id takes no pointer; the handle is owned below.
    let Ok(raw) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return false;
    };
    // SAFETY: `raw` is the process handle just opened, which nothing else owns.
    let process = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    created(HANDLE(process.as_raw_handle())) == Some(guard.version)
}

/// Grants each of `sids` `PROCESS_QUERY_LIMITED_INFORMATION` on this process, and nothing else,
/// by adding allow entries to its DACL, so the bridge can run [`server_is`] on it.
pub(crate) fn open_to(sids: &[&Sid]) -> io::Result<()> {
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing, with every right
    // on this process, READ_CONTROL and WRITE_DAC included.
    let process = unsafe { GetCurrentProcess() };
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `process` is live; `dacl` and `descriptor` are live for Windows to fill, the
    // descriptor in memory it allocates and freed below, `dacl` pointing into it.
    let status = unsafe {
        GetSecurityInfo(
            process,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&raw mut dacl),
            None,
            Some(&raw mut descriptor),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(code(status.0)));
    }
    let entries: Vec<EXPLICIT_ACCESS_W> = sids
        .iter()
        .map(|sid| EXPLICIT_ACCESS_W {
            grfAccessPermissions: PROCESS_QUERY_LIMITED_INFORMATION.0,
            grfAccessMode: GRANT_ACCESS,
            grfInheritance: NO_INHERITANCE,
            Trustee: TRUSTEE_W {
                pMultipleTrustee: std::ptr::null_mut(),
                MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_UNKNOWN,
                // With TRUSTEE_IS_SID the name field holds the SID; Windows only reads it.
                ptstrName: PWSTR(sid.as_psid().0.cast()),
            },
        })
        .collect();
    let mut merged: *mut ACL = std::ptr::null_mut();
    // SAFETY: `entries` name SIDs that outlive the call; `dacl` points into `descriptor`, still
    // alive; `merged` is live for Windows to fill with an ACL it allocates, freed below.
    let status = unsafe { SetEntriesInAclW(Some(&entries), Some(dacl), &raw mut merged) };
    // SAFETY: allocated by GetSecurityInfo with LocalAlloc, freed once; `dacl` is not used after.
    let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(code(status.0)));
    }
    // SAFETY: `process` is live with WRITE_DAC and `merged` is a valid ACL until freed below.
    let status = unsafe {
        SetSecurityInfo(
            process,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(merged.cast_const()),
            None,
        )
    };
    // SAFETY: allocated by SetEntriesInAclW with LocalAlloc, freed once, not used after.
    let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(merged.cast()))) };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code(status.0)))
    }
}

/// A Win32 error code as `from_raw_os_error` takes it.
fn code(raw: u32) -> i32 {
    i32::try_from(raw).unwrap_or(i32::MAX)
}
