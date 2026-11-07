//! The sweep's calls on Windows. The list is `WTSEnumerateProcessesW`'s: each process with its
//! token's user, read by the service, so the outer need open no process to learn whose it is; a
//! process it gives no user for is [`Owner::None`], passed over. A
//! process is opened with `PROCESS_TERMINATE`, `PROCESS_QUERY_LIMITED_INFORMATION` and
//! `SYNCHRONIZE`, the rights the agent's default DACL gives the desktop user, then checked with
//! `IsProcessInJob` and ended with `TerminateProcess`.
#![expect(unsafe_code, reason = "FFI to the process list, process and job calls")]

use std::io;
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::JobObjects::IsProcessInJob;
use windows::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_PROCESS_INFOW, WTSEnumerateProcessesW, WTSFreeMemory,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    TerminateProcess, WaitForSingleObject,
};
use windows::core::BOOL;

use super::{Owner, Processes, Seen};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::token::os::sid_text;

/// How long an ended process has to be gone.
const KILL_WAIT: Duration = Duration::from_secs(5);
/// `ERROR_INVALID_PARAMETER`: `OpenProcess` of a pid no process has.
const NO_SUCH_PID: i32 = 87;
/// The exit code a swept process ends with: the runner's `RunExit` range's last code.
const SWEPT: u32 = super::super::RUN_EXIT_BASE + 0xff;

/// The calls, against this launch's kill-on-close job.
pub(in crate::windows::runner) struct OsProcesses<'a> {
    pub(in crate::windows::runner) job: &'a Handle,
}

/// `WTSEnumerateProcessesW`'s list, freed on drop.
struct List(*mut WTS_PROCESS_INFOW);

impl Drop for List {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocated by WTSEnumerateProcessesW and freed once, here.
            unsafe { WTSFreeMemory(self.0.cast()) };
        }
    }
}

impl Processes for OsProcesses<'_> {
    type Process = Handle;

    fn list(&mut self) -> io::Result<Vec<Seen>> {
        let mut info: *mut WTS_PROCESS_INFOW = std::ptr::null_mut();
        let mut count = 0u32;
        // SAFETY: level 1 with live out-pointers; the list is freed when `list` drops.
        unsafe {
            WTSEnumerateProcessesW(
                Some(WTS_CURRENT_SERVER_HANDLE),
                0,
                1,
                &raw mut info,
                &raw mut count,
            )
        }
        .map_err(|e| os_error(&e))?;
        let list = List(info);
        if list.0.is_null() {
            return Err(io::Error::other("the process list is empty"));
        }
        let len = usize::try_from(count).map_err(io::Error::other)?;
        // SAFETY: the call wrote `count` entries at `info`, live until `list` drops.
        let entries = unsafe { std::slice::from_raw_parts(list.0, len) };
        entries.iter().map(seen).collect()
    }

    fn open(&mut self, pid: u32) -> io::Result<Option<Handle>> {
        let access = PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
        // SAFETY: a plain open by pid; the handle is owned by `Handle`.
        match unsafe { OpenProcess(access, false, pid) } {
            Ok(handle) => Ok(Some(Handle(handle))),
            Err(error) if os_error(&error).raw_os_error() == Some(NO_SUCH_PID) => Ok(None),
            Err(error) => Err(os_error(&error)),
        }
    }

    fn in_job(&mut self, process: &Handle) -> io::Result<bool> {
        let mut inside = BOOL(0);
        // SAFETY: both handles are live; `inside` is a live out-pointer.
        unsafe { IsProcessInJob(process.0, Some(self.job.0), &raw mut inside) }
            .map_err(|e| os_error(&e))?;
        Ok(inside.as_bool())
    }

    fn kill(&mut self, process: &Handle) -> io::Result<()> {
        // SAFETY: the handle is live with PROCESS_TERMINATE.
        let ended = unsafe { TerminateProcess(process.0, SWEPT) }.map_err(|e| os_error(&e));
        if let Err(error) = ended {
            // One already exiting refuses the call; it is gone all the same.
            return if exited(process.0, Duration::ZERO) {
                Ok(())
            } else {
                Err(error)
            };
        }
        if exited(process.0, KILL_WAIT) {
            Ok(())
        } else {
            Err(io::Error::from(io::ErrorKind::TimedOut))
        }
    }
}

/// One entry of the list; a SID the service gave that isn't one fails the list.
fn seen(entry: &WTS_PROCESS_INFOW) -> io::Result<Seen> {
    let image = if entry.pProcessName.is_null() {
        String::from("?")
    } else {
        // SAFETY: the service wrote a NUL-terminated name, live while the list is.
        unsafe { entry.pProcessName.to_string() }.unwrap_or_else(|_| String::from("?"))
    };
    // No SID: one of the kernel's or a protected process, never a pool user's (`Owner::None`).
    let owner = if entry.pUserSid.is_invalid() {
        Owner::None
    } else {
        Owner::User(sid_text(entry.pUserSid)?)
    };
    Ok(Seen {
        pid: entry.ProcessId,
        image,
        owner,
    })
}

/// Whether `process` has ended within `wait`.
fn exited(process: HANDLE, wait: Duration) -> bool {
    let millis = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the handle is live with SYNCHRONIZE.
    unsafe { WaitForSingleObject(process, millis) == WAIT_OBJECT_0 }
}
