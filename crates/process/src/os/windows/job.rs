//! The job's process list and the handles read from it: the ids `JobObjectBasicProcessIdList`
//! gives, each opened and confirmed in the job, and whether a process still runs. A child of
//! `windows.rs`, whose `unsafe_code` expectation covers its Win32 calls.

use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::sync::{Arc, Mutex, PoisonError};

use windows::Win32::Foundation::{ERROR_MORE_DATA, WAIT_TIMEOUT};
use windows::Win32::System::JobObjects::{
    IsProcessInJob, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JobObjectBasicAccountingInformation, JobObjectBasicProcessIdList, QueryInformationJobObject,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    WaitForSingleObject,
};

use super::{each_process, raw};

/// The first job query's room, in pids; a short answer retries with more, up to [`MAX_JOB_PIDS`].
const FIRST_JOB_PIDS: usize = 64;
/// The most processes of one job the listing reads.
const MAX_JOB_PIDS: usize = 65_536;

/// Handles (`SYNCHRONIZE`, query) to processes listed in a job, by pid. A held handle keeps its
/// pid from being reused, so a pid held is never opened again.
pub(super) type Held = Arc<Mutex<Vec<(u32, OwnedHandle)>>>;

/// Opens every process `job` lists that `held` does not hold yet, and keeps each one that is in
/// the job (a pid that ended after the list was read may name a process outside it now, which is
/// never held). The watch reads the list again at each check that finds the job's count at 0, so
/// a process started since is held too. A failed query adds nothing.
pub(super) fn hold_listed(job: &OwnedHandle, held: &Held) {
    let Some(pids) = job_pids(job) else {
        return;
    };
    let mut held = held.lock().unwrap_or_else(PoisonError::into_inner);
    for pid in pids {
        if held.iter().any(|&(known, _)| known == pid) {
            continue;
        }
        let Some(process) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)
        else {
            continue;
        };
        if in_job(&process, job) {
            held.push((pid, process));
        }
    }
}

/// Holds every process the system lists (a Toolhelp snapshot) that is in `job` and `held` does
/// not hold yet. For the kill, when a process joined the job between the listing and
/// `TerminateJobObject`: the job's own list may leave out a process the terminate has marked
/// ended, and the system's still shows each process until it has exited. A failed snapshot adds
/// nothing.
pub(super) fn hold_in_job(job: &OwnedHandle, held: &Held) {
    let mut pids = Vec::new();
    if !each_process(|entry| pids.push(entry.th32ProcessID)) {
        return;
    }
    let mut held = held.lock().unwrap_or_else(PoisonError::into_inner);
    for pid in pids {
        if held.iter().any(|&(known, _)| known == pid) {
            continue;
        }
        let Some(process) = open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)
        else {
            continue;
        };
        if in_job(&process, job) {
            held.push((pid, process));
        }
    }
}

/// The job's count of running processes and of every process it has held, from
/// `JobObjectBasicAccountingInformation`; `None` when the query fails.
pub(super) fn accounting(job: &OwnedHandle) -> Option<(u32, u32)> {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let size = u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>()).ok()?;
    // SAFETY: `info` is a live JOBOBJECT_BASIC_ACCOUNTING_INFORMATION of `size` bytes, the
    // struct this information class writes, and `job` is a live job handle.
    unsafe {
        QueryInformationJobObject(
            Some(raw(job)),
            JobObjectBasicAccountingInformation,
            (&raw mut info).cast(),
            size,
            None,
        )
    }
    .ok()?;
    Some((info.ActiveProcesses, info.TotalProcesses))
}

/// Whether `process` is in `job`; `false` when Windows does not say.
pub(super) fn in_job(process: &OwnedHandle, job: &OwnedHandle) -> bool {
    let mut inside = windows::core::BOOL(0);
    // SAFETY: `process` is a live handle with PROCESS_QUERY_LIMITED_INFORMATION, `job` the live
    // job handle, and `inside` a live BOOL the call writes.
    let asked = unsafe { IsProcessInJob(raw(process), Some(raw(job)), &raw mut inside) };
    asked.is_ok() && inside.as_bool()
}

/// Whether `process` still runs: a process handle is signalled once the process exits, so a
/// wait of 0 ms that times out means it has not. Unlike an exit code of `STILL_ACTIVE`, which a
/// process can also exit with, this cannot mistake an ended process for a running one.
pub(super) fn alive(process: &OwnedHandle) -> bool {
    // SAFETY: `process` is a live process handle opened with PROCESS_SYNCHRONIZE, which a wait
    // needs; a 0 ms wait only reads its state.
    let waited = unsafe { WaitForSingleObject(raw(process), 0) };
    waited == WAIT_TIMEOUT
}

/// The ids of the processes in `job`, from `JobObjectBasicProcessIdList`; `None` when the
/// query fails, or the job holds more than [`MAX_JOB_PIDS`].
pub(super) fn job_pids(job: &OwnedHandle) -> Option<Vec<u32>> {
    // The list's header (two u32 counts) in `usize` slots: the ids start at this slot.
    let header =
        std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
    let mut room = FIRST_JOB_PIDS;
    loop {
        // `usize` slots, so the buffer has the struct's alignment; zeroed, so it is all
        // initialised whatever the call writes.
        let mut buffer: Vec<usize> = vec![0; header + room];
        let bytes = u32::try_from(buffer.len() * size_of::<usize>()).ok()?;
        // SAFETY: `buffer` is live, aligned for JOBOBJECT_BASIC_PROCESS_ID_LIST and `bytes`
        // bytes long, which is at least the struct's size (`room` >= 1); the call writes the
        // two counts and at most the ids that fit in `bytes`. `job` is the live job handle.
        let queried = unsafe {
            QueryInformationJobObject(
                Some(raw(job)),
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr().cast(),
                bytes,
                None,
            )
        };
        if let Err(error) = &queried
            && error.code() != ERROR_MORE_DATA.to_hresult()
        {
            return None;
        }
        // SAFETY: the buffer starts with the struct's two u32 counts (repr(C), at offset 0),
        // initialised either by the call or by the zeroing above.
        let (assigned, listed) = unsafe {
            let list = buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
            (
                (*list).NumberOfAssignedProcesses,
                (*list).NumberOfProcessIdsInList,
            )
        };
        let (assigned, listed) = (
            usize::try_from(assigned).ok()?,
            usize::try_from(listed).ok()?,
        );
        if queried.is_ok() && listed >= assigned {
            let ids = buffer.get(header..header + listed.min(room))?;
            return Some(
                ids.iter()
                    .filter_map(|&id| u32::try_from(id).ok())
                    .collect(),
            );
        }
        if room >= MAX_JOB_PIDS {
            return None;
        }
        room = (room * 2).max(assigned + 8).min(MAX_JOB_PIDS);
    }
}

/// Opens process `pid` with `access`; `None` when it is gone or refuses.
pub(super) fn open(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> Option<OwnedHandle> {
    // SAFETY: opening a process by id takes no pointer.
    let handle = unsafe { OpenProcess(access, false, pid) }.ok()?;
    // SAFETY: `handle` is the open handle OpenProcess just returned, which nothing else owns;
    // the `OwnedHandle` closes it once, on drop.
    Some(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}
