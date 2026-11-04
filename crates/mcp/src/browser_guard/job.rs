//! Windows only: agent-browser's tree in a Job Object the guard owns.
//!
//! agent-browser starts its daemon with `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`
//! (`connection.rs:963-983`), and the daemon starts Chrome; killing the direct child leaves
//! both. So the guard starts agent-browser suspended, puts it in a new job with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (and no breakaway allowed, so nothing it starts can
//! leave), and only then resumes it. Ending the job ends the daemon and Chrome; so does the
//! job's last handle closing when the guard exits, however it exits. The guard itself runs in
//! the agent's job (`clauseo-process`); jobs nest.
//!
//! `clauseo-process` has the same steps, but for its async child and with dependencies this
//! crate may not take (`ARCHITECTURE.md`), so the few calls are repeated here.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 Job Object and thread APIs, for agent-browser's tree"
)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Threading::{
    CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME,
};
use windows::core::PCWSTR;

/// The creation flag that starts agent-browser suspended, for [`Job::adopt`].
pub(crate) const START_SUSPENDED: u32 = CREATE_SUSPENDED.0;

/// The exit code the job's end gives its processes.
const TERMINATED: u32 = 1;

/// agent-browser's job. Dropping it closes the handle, which ends what is left of the tree.
#[derive(Debug)]
pub(crate) struct Job {
    job: OwnedHandle,
}

impl Job {
    /// The job of `child`, just started with [`START_SUSPENDED`]: a new job holds it, then its
    /// threads resume. When a step fails the child is killed (still suspended, it started
    /// nothing) and the error returned: agent-browser never runs outside the job.
    pub(crate) fn adopt(child: &mut Child) -> io::Result<Job> {
        let job = match Job::new() {
            Ok(job) => job,
            Err(error) => {
                drop(child.kill());
                return Err(error);
            }
        };
        let process = HANDLE(child.as_raw_handle());
        // SAFETY: `job.handle()` is the job handle `job` owns, and `process` the child's own
        // process handle, open while `child` lives, with the full rights std opens it with.
        if let Err(error) = unsafe { AssignProcessToJobObject(job.handle(), process) } {
            drop(child.kill());
            return Err(io::Error::other(format!(
                "could not put agent-browser in its job: {error}"
            )));
        }
        if let Err(error) = resume(child.id()) {
            job.kill();
            return Err(error);
        }
        Ok(job)
    }

    /// Ends every process in the job: agent-browser, its daemon and Chrome. Returns whether
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
    fn new() -> io::Result<Job> {
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

/// Resumes every thread of process `pid`, which started suspended. Std gives no thread handle,
/// so the threads are found with a Toolhelp snapshot. Fails unless at least one thread was
/// suspended and resumed.
fn resume(pid: u32) -> io::Result<()> {
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(size_of::<THREADENTRY32>()).map_err(io::Error::other)?,
        ..THREADENTRY32::default()
    };
    // SAFETY: a thread snapshot of the whole system takes no pointer; its handle is closed
    // below, and nothing between here and there returns early.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|error| io::Error::other(format!("could not list the threads: {error}")))?;
    let mut resumed = 0_usize;
    // SAFETY: `snapshot` is the live snapshot handle and `entry` a THREADENTRY32 whose `dwSize`
    // is set, as Thread32First and Thread32Next require.
    let mut next = unsafe { Thread32First(snapshot, &raw mut entry) };
    while next.is_ok() {
        if entry.th32OwnerProcessID == pid && resume_thread(entry.th32ThreadID) {
            resumed += 1;
        }
        // SAFETY: as for Thread32First above.
        next = unsafe { Thread32Next(snapshot, &raw mut entry) };
    }
    // SAFETY: `snapshot` is a handle this function owns and closes only here.
    drop(unsafe { CloseHandle(snapshot) });
    if resumed == 0 {
        return Err(io::Error::other(
            "agent-browser started suspended and no thread of it resumed",
        ));
    }
    Ok(())
}

/// Resumes thread `id`; whether it was suspended and now runs. `ResumeThread` returns the
/// previous suspend count, `u32::MAX` on error.
fn resume_thread(id: u32) -> bool {
    // SAFETY: opening a thread by id takes no pointer; the handle is closed below.
    let Ok(thread) = (unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, id) }) else {
        return false;
    };
    // SAFETY: `thread` is a live handle opened with THREAD_SUSPEND_RESUME.
    let count = unsafe { ResumeThread(thread) };
    // SAFETY: `thread` is a handle this function owns and closes only here.
    drop(unsafe { CloseHandle(thread) });
    count != u32::MAX && count != 0
}
