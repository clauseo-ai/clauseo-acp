//! The sweep's calls on Windows, made as the pool user. The list is a Toolhelp snapshot
//! (`CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)`, which "includes all processes in the
//! system", every session's), and each process's user is read by opening it with
//! `PROCESS_QUERY_LIMITED_INFORMATION` and its token with `TOKEN_QUERY`: the pool user's own
//! processes open for it, since their default DACL grants their user full access. A process it
//! can't open, or whose token it can't, is [`Owner::None`], passed over. `WTSEnumerateProcessesW`
//! isn't used: Microsoft documents that "the caller must be a member of the Administrators group
//! to enumerate processes that are running under a different user's context", and a service's
//! session-0 process of the pool user's is the one the sweep is for. A process is opened with
//! `PROCESS_TERMINATE`, `PROCESS_QUERY_LIMITED_INFORMATION` and `SYNCHRONIZE`, checked with
//! `IsProcessInJob` against the job handle the outer gave the inner (`JOB_OBJECT_QUERY`), and
//! ended with `TerminateProcess`.
#![expect(
    unsafe_code,
    reason = "FFI to the process list, process, token and job calls"
)]

use std::io;
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::TOKEN_QUERY;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::JobObjects::IsProcessInJob;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use windows::core::BOOL;

use super::{Owner, Point, Processes, Seen, SweepError, sweep};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::sids::Sid;

/// How long an ended process has to be gone.
const KILL_WAIT: Duration = Duration::from_secs(5);
/// `ERROR_INVALID_PARAMETER`: `OpenProcess` of a pid no process has.
const NO_SUCH_PID: i32 = 87;
/// `ERROR_NO_MORE_FILES`: the snapshot's end.
const NO_MORE: i32 = 18;
/// The exit code a swept process ends with: the runner's `RunExit` range's last code.
const SWEPT: u32 = super::super::RUN_EXIT_BASE + 0xff;

/// The calls, against this launch's kill-on-close job when there is one.
pub(in crate::windows::runner) struct OsProcesses {
    /// The job handle the outer duplicated into the inner (`JOB_OBJECT_QUERY`), or `None` for
    /// a sweep that keeps no job's processes.
    pub(in crate::windows::runner) job: Option<HANDLE>,
}

/// Sweeps the processes of `me` (this process's own user) at `point`, keeping this process and,
/// with `job`, the job's processes. Each kill is a line given to `log`.
pub(in crate::windows::runner) fn sweep_own(
    me: &Sid,
    job: Option<HANDLE>,
    point: Point,
    log: &mut dyn FnMut(&str),
) -> Result<(), SweepError> {
    // SAFETY: no arguments.
    let own = unsafe { GetCurrentProcessId() };
    let mut calls = OsProcesses { job };
    sweep(&mut calls, me, &[own], point, log).map(drop)
}

impl Processes for OsProcesses {
    type Process = Handle;

    fn list(&mut self) -> io::Result<Vec<Seen>> {
        // SAFETY: a snapshot of every process; the handle is owned by `Handle`.
        let snapshot = Handle(
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.map_err(|e| os_error(&e))?,
        );
        let mut entry = PROCESSENTRY32W {
            dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).map_err(io::Error::other)?,
            ..PROCESSENTRY32W::default()
        };
        let mut seen = Vec::new();
        // SAFETY: the snapshot is live; `entry` is a live PROCESSENTRY32W with its size set.
        let mut next = unsafe { Process32FirstW(snapshot.0, &raw mut entry) };
        loop {
            match next {
                Ok(()) => seen.push(Seen {
                    pid: entry.th32ProcessID,
                    image: image(&entry.szExeFile),
                    owner: owner(entry.th32ProcessID),
                }),
                Err(error) if os_error(&error).raw_os_error() == Some(NO_MORE) => return Ok(seen),
                Err(error) => return Err(os_error(&error)),
            }
            // SAFETY: as for the first entry.
            next = unsafe { Process32NextW(snapshot.0, &raw mut entry) };
        }
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
        let Some(job) = self.job else {
            return Ok(false);
        };
        let mut inside = BOOL(0);
        // SAFETY: both handles are live; `inside` is a live out-pointer. The job handle is never
        // null here: a null one would ask about any job.
        unsafe { IsProcessInJob(process.0, Some(job), &raw mut inside) }
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

/// The image's file name the snapshot gave, up to its NUL.
fn image(name: &[u16]) -> String {
    let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
    String::from_utf16(name.get(..end).unwrap_or_default()).unwrap_or_else(|_| String::from("?"))
}

/// `pid`'s token user, or [`Owner::None`] when this process can't read it.
fn owner(pid: u32) -> Owner {
    // SAFETY: a plain open by pid; the handle is owned by `Handle`.
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
    else {
        return Owner::None;
    };
    let process = Handle(process);
    let mut token = HANDLE::default();
    // SAFETY: the process handle is live with PROCESS_QUERY_LIMITED_INFORMATION; `token` is a
    // live out-pointer, owned by `Handle` once written.
    if unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &raw mut token) }.is_err() {
        return Owner::None;
    }
    crate::windows::token::os::user(&Handle(token)).map_or(Owner::None, Owner::User)
}

/// Whether `process` has ended within `wait`.
fn exited(process: HANDLE, wait: Duration) -> bool {
    let millis = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the handle is live with SYNCHRONIZE.
    unsafe { WaitForSingleObject(process, millis) == WAIT_OBJECT_0 }
}
