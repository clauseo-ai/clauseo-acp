//! Windows only: the interpreter in a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so
//! killing `pdftoppm.exe`, `pdfinfo.exe` or `pdftotext.exe` (Claude Code's 120 s timeout) ends
//! `python.exe` too, and the tesseract processes pdftotext's script starts, which are in the job
//! as its children: the job's only handle is the launcher's, and Windows closes it however the
//! launcher ends.
//!
//! The child goes into the job right after it starts (std's `Command` cannot start it
//! suspended). pdftoppm's and pdfinfo's script starts no process; pdftotext's starts tesseract
//! only after it has loaded and read the PDF, long after that moment, so nothing escapes it.
//! The calls are those of `clauseo-mcp`'s `browser_guard/job.rs`, repeated for a std child.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 Job Object APIs, for the interpreter the launcher starts"
)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::core::PCWSTR;

/// A job that ends its processes when its last handle closes. Kept until the child exits;
/// dropping it closes the handle.
#[derive(Debug)]
pub(crate) struct Job {
    job: OwnedHandle,
}

impl Job {
    /// A new job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    pub(crate) fn new() -> io::Result<Self> {
        // SAFETY: no security attributes and no name are valid arguments; the handle returned
        // is owned by the `Job` built from it, which closes it once.
        let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| io::Error::other(format!("could not create a job: {error}")))?;
        // SAFETY: `raw` is the open handle CreateJobObjectW just returned, which nothing else
        // owns; the `OwnedHandle` closes it once, on drop.
        let owned = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let job = Self { job: owned };
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

    /// Puts `child` in the job.
    pub(crate) fn assign(&self, child: &Child) -> io::Result<()> {
        let process = HANDLE(child.as_raw_handle());
        // SAFETY: both handles are open: the job's until `self` drops, the process's until
        // `child` drops, and both outlive this call.
        unsafe { AssignProcessToJobObject(self.handle(), process) }
            .map_err(|error| io::Error::other(format!("could not assign the job: {error}")))
    }

    fn handle(&self) -> HANDLE {
        HANDLE(self.job.as_raw_handle())
    }
}
