//! Windows only: Chrome and agent-browser's tree in one Job Object the guard owns.
//!
//! agent-browser starts its daemon with `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`
//! (`connection.rs:963-983`); killing the direct child leaves it. So the guard makes a job with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (and no breakaway allowed, so nothing in it can start a
//! process outside it) and creates Chrome (`chrome.rs`), agent-browser and each one-shot
//! agent-browser (`relay/`) in it ([`Job::spawn`], `job/spawn.rs`): each is in the job from its
//! creation, so a guard killed at any moment leaves none outside it. Ending the job ends Chrome,
//! agent-browser and its daemon; so does the job's last handle closing when the guard exits,
//! however it exits. The guard itself runs in the agent's job (`clauseo-process`); jobs nest.
//!
//! `clauseo-process` makes its job the same way, but for its async child and with dependencies
//! this crate may not take (`ARCHITECTURE.md`), so the few job calls are repeated here.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 Job Object APIs, for agent-browser's tree"
)]

mod child;
mod spawn;

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows::core::PCWSTR;

pub(crate) use child::{Child, ChildStderr, ChildStdin};
pub(crate) use spawn::{Io, Launch};

/// The exit code the job's end gives its processes.
const TERMINATED: u32 = 1;

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

    fn handle(&self) -> HANDLE {
        HANDLE(self.job.as_raw_handle())
    }

    /// A new job that ends its processes when its last handle closes. No breakaway flag is
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
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
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

#[cfg(test)]
mod tests;
