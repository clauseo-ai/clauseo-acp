//! The sweep's calls on Windows, made as the pool user.
//!
//! - [`Processes::list`]: a Toolhelp snapshot (`CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)`,
//!   which "includes all processes in the system", every session's), each process's user read by
//!   opening it with `PROCESS_QUERY_LIMITED_INFORMATION` and its token with `TOKEN_QUERY`. A
//!   process it can't open, or whose token it can't, is [`Owner::None`].
//! - [`Processes::named`]: `WTSEnumerateProcessesW` (level 1), whose service reads each token
//!   itself, so a process whose DACL shuts its user out still comes with its user. Microsoft
//!   documents that "the caller must be a member of the Administrators group to enumerate
//!   processes that are running under a different user's context" (the call's Remarks): the
//!   rule is about other users' processes, and this call is made as the pool user for the pool
//!   user's own. An entry with no SID (the kernel's and protected processes) is
//!   [`Owner::None`]. A list that fails fails the sweep.
//!
//! A process is opened with `PROCESS_TERMINATE`, `PROCESS_QUERY_LIMITED_INFORMATION` and
//! `SYNCHRONIZE`. One whose DACL refuses that (error 5) is opened with `WRITE_DAC`, which its
//! owner (this user) holds unless an `OWNER RIGHTS` entry takes it away, given a DACL granting
//! this user those rights, and opened again; when that fails too, the first error stands. Its
//! creation time is read with `GetProcessTimes`, and it is ended with `TerminateProcess`. Whether
//! one whose kill was refused has exited is `WaitForSingleObject` with no wait.
//!
//! [`Processes::job_pids`] is `QueryInformationJobObject` with no job handle, which "uses the
//! job associated with the calling process" (the immediate one, the launch's, when jobs nest),
//! for `JobObjectBasicProcessIdList`, asked again with a bigger buffer while the list doesn't fit
//! (`own_job_pids`). The inner holds no handle to its job, so the outer's is the job's last and
//! kill-on-close fires with the outer.
#![expect(
    unsafe_code,
    reason = "FFI to the process lists, process, token and job calls"
)]

use std::io;
use std::time::Duration;

use windows::Win32::Foundation::{FILETIME, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Security::TOKEN_QUERY;
use windows::Win32::Storage::FileSystem::WRITE_DAC;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::JobObjects::{
    JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList, QueryInformationJobObject,
};
use windows::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_PROCESS_INFOW, WTSEnumerateProcessesW, WTSFreeMemory,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcess, OpenProcessToken,
    PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};

use super::{Owner, Point, Processes, SWEPT, Seen, SweepError, sweep};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::sids::Sid;
use crate::windows::token::os::{shut_object, sid_text};

/// How long an ended process has to be gone.
const KILL_WAIT: Duration = Duration::from_secs(5);
/// `ERROR_INVALID_PARAMETER`: `OpenProcess` of a pid no process has.
const NO_SUCH_PID: i32 = 87;
/// `ERROR_NO_MORE_FILES`: the snapshot's end.
const NO_MORE: i32 = 18;
/// `ERROR_ACCESS_DENIED`.
const DENIED: i32 = 5;
/// `ERROR_MORE_DATA`: the job's pid list doesn't fit the buffer.
const MORE_DATA: i32 = 234;
/// The most pids the job's list is read for: past it, the read fails.
const JOB_PIDS_CAP: usize = 1 << 16;
/// The room, in pids, of the first read of the job's list.
const JOB_PIDS_FIRST: usize = 64;
/// The room added to the latest count when the job's list is read again.
const JOB_PIDS_HEADROOM: usize = 64;
/// The most calls one read of the job's list makes: the first and three more.
const JOB_PIDS_TRIES: usize = 4;

/// The calls.
pub(in crate::windows::runner) struct OsProcesses<'a> {
    /// This process's user, whom a DACL it rewrites grants.
    pub(in crate::windows::runner) me: &'a Sid,
}

/// `WTSEnumerateProcessesW`'s list, freed on drop.
struct WtsList(*mut WTS_PROCESS_INFOW);

impl Drop for WtsList {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocated by WTSEnumerateProcessesW and freed once, here.
            unsafe { WTSFreeMemory(self.0.cast()) };
        }
    }
}

/// Sweeps the processes of `me` (this process's own user) at `point`, keeping this process and,
/// past the first act, its job's processes. Each kill is a line given to `log`.
pub(in crate::windows::runner) fn sweep_own(
    me: &Sid,
    point: Point,
    log: &mut dyn FnMut(&str),
) -> Result<(), SweepError> {
    // SAFETY: no arguments.
    let own = unsafe { GetCurrentProcessId() };
    let mut calls = OsProcesses { me };
    sweep(&mut calls, me, &[own], point, log).map(drop)
}

impl Processes for OsProcesses<'_> {
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

    fn named(&mut self) -> io::Result<Vec<Seen>> {
        let mut info: *mut WTS_PROCESS_INFOW = std::ptr::null_mut();
        let mut count = 0u32;
        // SAFETY: version 1 with live out-pointers; the list is freed when `list` drops.
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
        let list = WtsList(info);
        if list.0.is_null() {
            return Err(io::Error::other("the process list is empty"));
        }
        let len = usize::try_from(count).map_err(io::Error::other)?;
        // SAFETY: the call wrote `count` entries at `info`, live until `list` drops.
        let entries = unsafe { std::slice::from_raw_parts(list.0, len) };
        entries.iter().map(named).collect()
    }

    fn open(&mut self, pid: u32) -> io::Result<Option<Handle>> {
        let access = PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE;
        let error = match open(access, pid) {
            Err(error) if error.raw_os_error() == Some(DENIED) => error,
            other => return other,
        };
        // As its owner: `WRITE_DAC`, unless an `OWNER RIGHTS` entry takes it away.
        let Ok(Some(writable)) = open(PROCESS_ACCESS_RIGHTS(WRITE_DAC.0), pid) else {
            return Err(error);
        };
        let grant = format!("D:P(A;;GA;;;SY)(A;;0x{:x};;;{})", access.0, self.me);
        if shut_object(writable.0, &grant).is_err() {
            return Err(error);
        }
        drop(writable);
        open(access, pid)
    }

    fn job_pids(&mut self) -> io::Result<Vec<u32>> {
        own_job_pids()
    }

    fn started(&mut self, process: &Handle) -> io::Result<u64> {
        times(process.0)
    }

    fn own_start(&mut self) -> io::Result<u64> {
        // SAFETY: the pseudo-handle of this process needs no close and carries every right.
        times(unsafe { GetCurrentProcess() })
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

    fn exited(&mut self, process: &Handle) -> bool {
        exited(process.0, Duration::ZERO)
    }

    fn pause(&mut self, pause: Duration) {
        sleep(pause);
    }
}

/// The pause between two looks at a process whose kill was refused.
#[expect(
    clippy::disallowed_methods,
    reason = "the sweep's bounded wait for a refused process to exit by itself, 100 ms at a time \
              for up to 2 s; the tests inject the pause (`refused::wait_out`)"
)]
fn sleep(pause: Duration) {
    std::thread::sleep(pause);
}

/// The pids of this process's own job (`JobObjectBasicProcessIdList`, no job handle), read by
/// [`job_pids_from`].
pub(in crate::windows::runner) fn own_job_pids() -> io::Result<Vec<u32>> {
    job_pids_from(query_own_job)
}

/// One query of the job's pid list.
pub(super) struct Asked {
    /// The call's result: `ERROR_MORE_DATA` when the list stopped at the buffer's end.
    pub(super) result: io::Result<()>,
    /// `NumberOfAssignedProcesses`: the job's `ActiveProcesses` as the call began. Windows
    /// writes it before the pids, so it is there with `ERROR_MORE_DATA` too.
    pub(super) assigned: usize,
    /// The pids written (`NumberOfProcessIdsInList` of them, at most `room`).
    pub(super) listed: Vec<usize>,
}

/// The job's pids, from `query` (given the room, in pids, its buffer has). A list that doesn't
/// fit (`ERROR_MORE_DATA`, or fewer pids than processes) or any failed call is asked again, up
/// to [`JOB_PIDS_TRIES`] calls, the room grown from the latest count plus
/// [`JOB_PIDS_HEADROOM`] (at least doubled), up to [`JOB_PIDS_CAP`]. A job filling up between
/// two calls (a fork bomb at the cap) so gets a bigger buffer each time. A list that still fails
/// is an error with the last Win32 code (`ERROR_MORE_DATA` for a short list), never an empty
/// list: the sweep then fails closed.
pub(super) fn job_pids_from(mut query: impl FnMut(usize) -> Asked) -> io::Result<Vec<u32>> {
    let mut room = JOB_PIDS_FIRST;
    let mut last = io::Error::from_raw_os_error(MORE_DATA);
    for _ in 0..JOB_PIDS_TRIES {
        let asked = query(room);
        match asked.result {
            Ok(()) if asked.assigned <= asked.listed.len() => {
                return asked
                    .listed
                    .iter()
                    .map(|pid| u32::try_from(*pid).map_err(io::Error::other))
                    .collect();
            }
            Ok(()) => last = io::Error::from_raw_os_error(MORE_DATA),
            Err(error) => last = error,
        }
        let wanted = asked
            .assigned
            .max(asked.listed.len())
            .saturating_add(JOB_PIDS_HEADROOM);
        if room >= JOB_PIDS_CAP {
            break;
        }
        room = wanted.max(room.saturating_mul(2)).min(JOB_PIDS_CAP);
    }
    Err(last)
}

/// The real query for [`job_pids_from`]: the caller's own job, into a buffer of `room` pids.
fn query_own_job(room: usize) -> Asked {
    let head = size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>().div_ceil(size_of::<usize>());
    // usize-aligned, as the list's pid array is.
    let mut buffer = vec![0usize; head + room];
    let Ok(len) = u32::try_from(size_of_val(buffer.as_slice())) else {
        return Asked {
            result: Err(io::Error::from_raw_os_error(MORE_DATA)),
            assigned: 0,
            listed: Vec::new(),
        };
    };
    // SAFETY: no job handle: the caller's own job. `buffer` is live, writable for `len`
    // bytes and aligned for the list.
    let result = unsafe {
        QueryInformationJobObject(
            None,
            JobObjectBasicProcessIdList,
            buffer.as_mut_ptr().cast(),
            len,
            None,
        )
    }
    .map_err(|e| os_error(&e));
    // SAFETY: the list's head is at the start of the aligned, zeroed buffer: written by the call,
    // or zeros when it wrote nothing.
    let list = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
    let assigned = usize::try_from(list.NumberOfAssignedProcesses).unwrap_or(usize::MAX);
    let count = usize::try_from(list.NumberOfProcessIdsInList)
        .unwrap_or(usize::MAX)
        .min(room);
    // SAFETY: `count` pids follow in `ProcessIdList`, inside `buffer` (`count <= room`).
    let pids = unsafe { std::slice::from_raw_parts(list.ProcessIdList.as_ptr(), count) };
    Asked {
        result,
        assigned,
        listed: pids.to_vec(),
    }
}

/// Opens `pid` with `access`; `None` when no process has that pid any more.
fn open(access: PROCESS_ACCESS_RIGHTS, pid: u32) -> io::Result<Option<Handle>> {
    // SAFETY: a plain open by pid; the handle is owned by `Handle`.
    match unsafe { OpenProcess(access, false, pid) } {
        Ok(handle) => Ok(Some(Handle(handle))),
        Err(error) if os_error(&error).raw_os_error() == Some(NO_SUCH_PID) => Ok(None),
        Err(error) => Err(os_error(&error)),
    }
}

/// `process`'s creation time, in 100 ns ticks.
fn times(process: HANDLE) -> io::Result<u64> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: the handle is live with PROCESS_QUERY_LIMITED_INFORMATION; four live out-pointers.
    unsafe {
        GetProcessTimes(
            process,
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    }
    .map_err(|e| os_error(&e))?;
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// One entry of WTS's list; a SID the service gave that isn't one fails the list.
fn named(entry: &WTS_PROCESS_INFOW) -> io::Result<Seen> {
    let image = if entry.pProcessName.is_null() {
        String::from("?")
    } else {
        // SAFETY: the service wrote a NUL-terminated name, live while the list is.
        unsafe { entry.pProcessName.to_string() }.unwrap_or_else(|_| String::from("?"))
    };
    // No SID: the kernel's or a protected process, never a pool user's.
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

#[cfg(test)]
mod tests;
