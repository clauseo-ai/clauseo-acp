//! The Win32 calls of the pool rows (`checks_pool.rs`), made by the harness as the lawyer,
//! outside the runner's job: every process whose token user is a pool user (a Toolhelp snapshot,
//! then `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` and `OpenProcessToken(TOKEN_QUERY)`),
//! the session's kill-on-close job (a duplicate of the outer runner's own handle to it),
//! `IsProcessInJob` against that job, its pid list, and a service's state. Each gives a plain
//! value or the Win32 code, never a panic.
#![expect(
    unsafe_code,
    reason = "FFI to the process, token, job, handle-duplication and service calls"
)]

use std::ffi::c_void;

use windows::Win32::Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE};
use windows::Win32::Security::TOKEN_QUERY;
use windows::Win32::System::JobObjects::{
    CreateJobObjectW, IsProcessInJob, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_BASIC_UI_RESTRICTIONS,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicProcessIdList,
    JobObjectBasicUIRestrictions, JobObjectExtendedLimitInformation, QueryInformationJobObject,
};
use windows::Win32::System::Services::{
    CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus, SC_HANDLE,
    SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS, SERVICE_STATUS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_DUP_HANDLE,
    PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::core::{BOOL, HSTRING, PCWSTR};

use super::win::{Owned, UI_LIMITS, code_of, token_user};
use super::{win_sec, win_thread};

/// `ERROR_MORE_DATA`: the job's pid list didn't fit.
const MORE_DATA: u32 = 234;

/// A process the snapshot found whose token user is the pool user, opened.
pub(crate) struct PoolProcess {
    /// Its pid.
    pub(crate) pid: u32,
    /// Its image's file name.
    pub(crate) image: String,
    /// The handle (`PROCESS_QUERY_LIMITED_INFORMATION`), for `IsProcessInJob`.
    pub(crate) handle: Owned,
}

/// What the listing found.
pub(crate) struct Listing {
    /// Every process whose token user is the pool user.
    pub(crate) found: Vec<PoolProcess>,
    /// How many processes the harness couldn't open, or whose token it couldn't open: their user
    /// is unknown.
    pub(crate) unknown: usize,
}

/// Every process whose token user is `sid` (no case), and how many couldn't be told; the
/// snapshot's code when it can't be taken.
pub(crate) fn pool_processes(sid: &str) -> Result<Listing, u32> {
    let mut listing = Listing {
        found: Vec::new(),
        unknown: 0,
    };
    for process in win_thread::processes()? {
        if process.pid == 0 || process.pid == 4 {
            continue;
        }
        // SAFETY: plain call; a handle that comes back is closed by `Owned`.
        let opened = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process.pid) };
        let Ok(handle) = opened.map(Owned) else {
            listing.unknown += 1;
            continue;
        };
        let mut token = HANDLE::default();
        // SAFETY: the process handle is live; `token` is a live out-pointer.
        if unsafe { OpenProcessToken(handle.0, TOKEN_QUERY, &raw mut token) }.is_err() {
            listing.unknown += 1;
            continue;
        }
        match token_user(Owned(token).0) {
            Ok(user) if user.eq_ignore_ascii_case(sid) => listing.found.push(PoolProcess {
                pid: process.pid,
                image: process.image,
                handle,
            }),
            Ok(_) => {}
            Err(_) => listing.unknown += 1,
        }
    }
    Ok(listing)
}

/// `IsProcessInJob(process, job)`; the code when it fails.
pub(crate) fn in_job(process: HANDLE, job: HANDLE) -> Result<bool, u32> {
    let mut inside = BOOL::default();
    // SAFETY: both handles are live; `inside` is a live out-pointer.
    unsafe { IsProcessInJob(process, Some(job), &raw mut inside) }.map_err(|e| code_of(&e))?;
    Ok(inside.as_bool())
}

/// Whether the process `pid` is in `job`: `IsProcessInJob` when the harness can open it, else
/// whether the job's pid list holds it. The call that failed and its code otherwise.
pub(crate) fn pid_in_job(pid: u32, job: HANDLE) -> Result<bool, String> {
    // SAFETY: plain call; a handle that comes back is closed by `Owned`.
    match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(handle) => in_job(Owned(handle).0, job).map_err(|c| format!("IsProcessInJob gave {c}")),
        Err(_) => job_pids(job)
            .map(|pids| pids.contains(&pid))
            .map_err(|c| format!("JobObjectBasicProcessIdList gave {c}")),
    }
}

/// The pids in `job` (`JobObjectBasicProcessIdList`); the code when the query fails.
pub(crate) fn job_pids(job: HANDLE) -> Result<Vec<u32>, u32> {
    let mut room = 256usize;
    loop {
        let head = size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        let mut buffer = vec![0usize; head.div_ceil(size_of::<usize>()) + room];
        let len = u32::try_from(size_of_val(buffer.as_slice())).map_err(|_big| MORE_DATA)?;
        // SAFETY: the job is live; `buffer` is live, writable, `len` bytes and aligned for the
        // list.
        let asked = unsafe {
            QueryInformationJobObject(
                Some(job),
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr().cast::<c_void>(),
                len,
                None,
            )
        };
        match asked.map_err(|e| code_of(&e)) {
            Ok(()) => {}
            Err(MORE_DATA) if room < 65_536 => {
                room *= 4;
                continue;
            }
            Err(code) => return Err(code),
        }
        // SAFETY: the call wrote the list at the start of the aligned buffer.
        let list = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
        let count = usize::try_from(list.NumberOfProcessIdsInList).unwrap_or(0);
        // SAFETY: `count` pids follow in `ProcessIdList`, inside `buffer`.
        let pids = unsafe { std::slice::from_raw_parts(list.ProcessIdList.as_ptr(), count) };
        return Ok(pids
            .iter()
            .filter_map(|pid| u32::try_from(*pid).ok())
            .collect());
    }
}

/// The session's kill-on-close job, duplicated from the outer runner `outer`'s handle table: the
/// one job handle there whose job kills on close and carries every UI limit ([`UI_LIMITS`]).
/// The outer creates it unnamed, so this is the only way to name it from outside.
///
/// The handle keeps the job alive: drop it before the session ends, or the agent's tree
/// outlives the outer runner.
pub(crate) fn session_job(outer: u32) -> Result<Owned, String> {
    let rights = PROCESS_DUP_HANDLE | PROCESS_QUERY_INFORMATION;
    // SAFETY: plain call; a handle that comes back is closed by `Owned`.
    let process = unsafe { OpenProcess(rights, false, outer) }
        .map(Owned)
        .map_err(|e| {
            format!(
                "OpenProcess(PROCESS_DUP_HANDLE) on the outer runner {outer} gave {}",
                code_of(&e)
            )
        })?;
    let job_type = job_type_index()?;
    let table = win_sec::handles_of(process.0)
        .map_err(|why| format!("the outer runner's handle table: {why}"))?;
    let mut jobs = Vec::new();
    for entry in table.iter().filter(|e| e.type_index == job_type) {
        let source = HANDLE(std::ptr::without_provenance_mut::<c_void>(entry.value));
        let mut copy = HANDLE::default();
        // SAFETY: `process` is live with PROCESS_DUP_HANDLE; `source` is a value from its
        // table; `copy` is a live out-pointer whose handle `Owned` closes.
        let duplicated = unsafe {
            DuplicateHandle(
                process.0,
                source,
                GetCurrentProcess(),
                &raw mut copy,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if duplicated.is_err() {
            continue;
        }
        let copy = Owned(copy);
        if ours(copy.0) {
            jobs.push(copy);
        }
    }
    let found = jobs.len();
    match jobs.pop() {
        Some(job) if found == 1 => Ok(job),
        Some(_) => Err(format!(
            "the outer runner {outer} holds {found} kill-on-close jobs with every UI limit"
        )),
        None => Err(format!(
            "the outer runner {outer} holds no kill-on-close job with every UI limit"
        )),
    }
}

/// The object type index of a job handle: read from a job this process makes (and closes).
fn job_type_index() -> Result<u32, String> {
    // SAFETY: no attributes and no name; the handle is closed by `Owned`.
    let probe = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map(Owned)
        .map_err(|e| format!("CreateJobObjectW gave {}", code_of(&e)))?;
    let value = probe.0.0.addr();
    win_sec::own_handles()?
        .iter()
        .find(|e| e.value == value)
        .map(|e| e.type_index)
        .ok_or_else(|| "a job of the harness's own isn't in its handle table".to_owned())
}

/// Whether `job` kills on close and carries every UI limit: the runner's kill-on-close job.
fn ours(job: HANDLE) -> bool {
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let mut ui = JOBOBJECT_BASIC_UI_RESTRICTIONS::default();
    let (Ok(limits_len), Ok(ui_len)) = (
        u32::try_from(size_of_val(&limits)),
        u32::try_from(size_of_val(&ui)),
    ) else {
        return false;
    };
    // SAFETY: the job is live; each struct is live, writable and of its length.
    let read = unsafe {
        QueryInformationJobObject(
            Some(job),
            JobObjectExtendedLimitInformation,
            (&raw mut limits).cast(),
            limits_len,
            None,
        )
        .and_then(|()| {
            QueryInformationJobObject(
                Some(job),
                JobObjectBasicUIRestrictions,
                (&raw mut ui).cast(),
                ui_len,
                None,
            )
        })
    };
    let kill = limits.BasicLimitInformation.LimitFlags.0 & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.0;
    read.is_ok() && kill != 0 && ui.UIRestrictionsClass.0 & UI_LIMITS == UI_LIMITS
}

/// A service handle closed on drop.
struct Service(SC_HANDLE);

impl Drop for Service {
    fn drop(&mut self) {
        // SAFETY: the handle is open and closed once, here.
        drop(unsafe { CloseServiceHandle(self.0) });
    }
}

/// The current state of the service `name` (`QueryServiceStatus`; 1 stopped, 4 running), read
/// through `OpenSCManagerW(SC_MANAGER_CONNECT)` and `OpenServiceW(SERVICE_QUERY_STATUS)`. The
/// call that failed and its code otherwise.
pub(crate) fn service_state(name: &str) -> Result<u32, String> {
    // SAFETY: no machine or database names; the handle is closed by `Service`.
    let manager = unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }
        .map(Service)
        .map_err(|e| format!("OpenSCManagerW gave {}", code_of(&e)))?;
    // SAFETY: the manager is live; the name is a live HSTRING; closed by `Service`.
    let service = unsafe { OpenServiceW(manager.0, &HSTRING::from(name), SERVICE_QUERY_STATUS) }
        .map(Service)
        .map_err(|e| format!("OpenServiceW gave {}", code_of(&e)))?;
    let mut status = SERVICE_STATUS::default();
    // SAFETY: the service is live; `status` is a live out-pointer.
    unsafe { QueryServiceStatus(service.0, &raw mut status) }
        .map_err(|e| format!("QueryServiceStatus gave {}", code_of(&e)))?;
    Ok(status.dwCurrentState.0)
}
