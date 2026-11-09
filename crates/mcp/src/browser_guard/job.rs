//! Windows only: Chrome and agent-browser's tree in one Job Object the guard owns.
//!
//! agent-browser starts its daemon with `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`
//! (`connection.rs:963-983`); killing the direct child leaves it. So the guard makes a job with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (and no breakaway allowed, so nothing in it can start a
//! process outside it), plus `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION` (a process that crashes
//! ends at once, its exit code the exception's, rather than hanging on Windows Error Reporting:
//! `chrome/postcheck/crashed.rs` reads that code, decision record 0029), and no UI restrictions
//! (Chrome's sandbox nests UI-restricted jobs of its own in it: `job/probe.rs`), and
//! creates Chrome
//! (`chrome.rs`), agent-browser and each one-shot agent-browser (`relay/`) in it ([`Job::spawn`],
//! `job/spawn.rs`): each is in the job from its creation, so a guard killed at any moment leaves
//! none outside it. Ending the job ends Chrome, agent-browser and its daemon; so does the job's
//! last handle closing when the guard exits, however it exits. The guard itself runs in the
//! agent's job (`clauseo-process`); jobs nest.
//!
//! `clauseo-process` makes its job the same way, but for its async child and with dependencies
//! this crate may not take (`ARCHITECTURE.md`), so the few job calls are repeated here.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 Job Object APIs, for agent-browser's tree"
)]

mod child;
mod probe;
mod spawn;

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{ERROR_MORE_DATA, HANDLE};
use windows::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};
use windows::core::PCWSTR;

pub(crate) use child::{Child, ChildStderr, ChildStdin};
pub(crate) use probe::{Nesting, ui_jobs_nest};
pub(crate) use spawn::{Io, Launch, windows_dirs};

/// The exit code the job's end gives its processes.
const TERMINATED: u32 = 1;
/// How many pids [`Job::process_ids`] makes room for first; a full list is asked for again with
/// twice the room, up to [`MAX_IDS`].
const FIRST_IDS: usize = 64;
/// The most processes listed; a job with more fails the listing.
const MAX_IDS: usize = 65_536;

/// The guard's job. Dropping it closes the handle, which ends what is left in it.
#[derive(Debug)]
pub(crate) struct Job {
    job: OwnedHandle,
}

impl Job {
    /// Starts `launch` in the job, running: nothing the guard starts is ever outside it
    /// (`job/spawn.rs`).
    pub(crate) fn spawn(&self, launch: &Launch<'_>) -> io::Result<Child> {
        spawn::spawn(self.handle(), launch)
    }

    /// Ends every process in the job: Chrome, agent-browser and its daemon. Returns whether
    /// Windows took the call.
    pub(crate) fn kill(&self) -> bool {
        // SAFETY: `self.handle()` is the job handle `self` owns, open until `self` drops.
        unsafe { TerminateJobObject(self.handle(), TERMINATED) }.is_ok()
    }

    /// The ids of the processes in the job, its nested jobs' included (Chrome's sandbox puts
    /// its children in jobs of its own, inside this one); `None` when Windows refuses the call
    /// or more than [`MAX_IDS`] run. For the check after Chrome's start (`chrome/postcheck.rs`).
    pub(crate) fn process_ids(&self) -> Option<Vec<u32>> {
        let header = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList);
        let header_words = header.div_ceil(size_of::<usize>());
        let mut room = FIRST_IDS;
        loop {
            // Whole `usize` words, so the list is aligned as JOBOBJECT_BASIC_PROCESS_ID_LIST is.
            let mut words = vec![0_usize; header_words + room];
            let bytes = u32::try_from(words.len() * size_of::<usize>()).ok()?;
            // SAFETY: `words` is a live, zeroed, `usize`-aligned buffer of exactly `bytes` bytes,
            // which Windows fills with a JOBOBJECT_BASIC_PROCESS_ID_LIST and writes no further;
            // `self.handle()` is the job handle `self` owns.
            let listed = unsafe {
                QueryInformationJobObject(
                    Some(self.handle()),
                    JobObjectBasicProcessIdList,
                    words.as_mut_ptr().cast(),
                    bytes,
                    None,
                )
            };
            let complete = match listed {
                Ok(()) => true,
                Err(error) if error.code() == ERROR_MORE_DATA.to_hresult() => false,
                Err(_) => return None,
            };
            // SAFETY: `words` is aligned for and at least as large as the struct's header,
            // which Windows wrote (zeroed before, so a valid value either way).
            let list = unsafe {
                words
                    .as_ptr()
                    .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>()
                    .read()
            };
            let (assigned, listed_ids) = (
                usize::try_from(list.NumberOfAssignedProcesses).ok()?,
                usize::try_from(list.NumberOfProcessIdsInList).ok()?,
            );
            if complete && listed_ids >= assigned && listed_ids <= room {
                return words
                    .get(header_words..header_words + listed_ids)?
                    .iter()
                    .map(|&id| u32::try_from(id).ok())
                    .collect();
            }
            if room >= MAX_IDS {
                return None;
            }
            room = (room * 2).max(assigned.min(MAX_IDS));
        }
    }

    fn handle(&self) -> HANDLE {
        HANDLE(self.job.as_raw_handle())
    }

    /// This job's UI restrictions (`JOBOBJECT_BASIC_UI_RESTRICTIONS`): always 0, since the guard
    /// never sets any (`job/probe.rs`); `None` when Windows refuses the call.
    #[cfg(test)]
    pub(crate) fn ui_restrictions(&self) -> Option<u32> {
        ui_restrictions(Some(self.handle()))
    }

    /// A new job that ends its processes when its last handle closes, and a process of it at
    /// once when it crashes (no Windows Error Reporting dialog or hang). No breakaway flag is
    /// set, so a process in it cannot start one outside it.
    pub(crate) fn new() -> io::Result<Job> {
        // SAFETY: no security attributes and no name are valid arguments; the handle returned
        // is owned by the `Job` built from it, which closes it once.
        let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| io::Error::other(format!("could not create a job: {error}")))?;
        // SAFETY: `raw` is the open handle CreateJobObjectW just returned, which nothing else
        // owns; the `OwnedHandle` closes it once, on drop.
        let owned = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let job = Job { job: owned };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // Die on an unhandled exception is required here, not only in the agent's job: this job
        // is nested in that one, and the innermost job's limit is the one that applies to its
        // processes. Measured on Windows 11 (stage C): with the flag on the agent's job only,
        // this job read 0x2000 (kill on close alone), Chrome's start crash (0x80000003) went to
        // Windows Error Reporting, and Chrome hung until the guard's 15 s wait ended.
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .map_err(io::Error::other)?;
        // SAFETY: `limits` is a live JOBOBJECT_EXTENDED_LIMIT_INFORMATION of `size` bytes, the
        // struct this information class takes, and `job.handle()` is the handle `job` owns.
        unsafe {
            SetInformationJobObject(
                job.handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            )
        }
        .map_err(|error| io::Error::other(format!("could not set the job's limit: {error}")))?;
        Ok(job)
    }
}

/// `job`'s UI restrictions.
#[cfg(test)]
fn ui_restrictions(job: Option<HANDLE>) -> Option<u32> {
    use windows::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_UI_RESTRICTIONS, JobObjectBasicUIRestrictions,
    };
    let mut limits = JOBOBJECT_BASIC_UI_RESTRICTIONS::default();
    let size = u32::try_from(size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>()).ok()?;
    // SAFETY: `limits` is a live JOBOBJECT_BASIC_UI_RESTRICTIONS of `size` bytes, the struct
    // this information class writes; `job` is a job handle its owner keeps open.
    unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicUIRestrictions,
            (&raw mut limits).cast(),
            size,
            None,
        )
    }
    .ok()?;
    Some(limits.UIRestrictionsClass.0)
}

#[cfg(test)]
mod tests;
