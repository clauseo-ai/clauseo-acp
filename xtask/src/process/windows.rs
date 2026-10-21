//! The Windows half of a command's tree: a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
//! as floor 2's `process` crate (S7) does. The command starts suspended, joins the job, and only
//! then runs, so nothing it starts can run outside the job. Closing the job's handle kills every
//! process in it, and Windows closes it when xtask dies, however it dies, so Windows needs no
//! leader. Unlike S7 the command does not get `CREATE_NO_WINDOW`: xtask runs in a console, and an
//! `Output::Inherit` command writes to it. Every Win32 call of xtask is in this file.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 Job Object, process and thread APIs"
)]

use super::running::{Streams, Tree};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
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

/// The exit code `TerminateJobObject` gives the processes it ends.
const TERMINATED: u32 = 1;

/// The Job Object a command runs in. Dropping it closes the handle, which kills what is left.
pub(super) struct Job {
    job: HANDLE,
}

impl Tree for Job {
    const NAME: &'static str = "job";

    /// Makes `command` start suspended, so it runs nothing before it joins its job. Its output
    /// goes where it goes: a job is no background group, so a command that inherits xtask's
    /// console can still prompt on it.
    fn prepare(command: &mut Command, _streams: Streams) {
        command.creation_flags(CREATE_SUSPENDED.0);
    }

    /// The tree of `child`, just started suspended from a `prepare`d command: a new job holds it,
    /// then its threads resume. When a step fails, the child is ended and the error returned; the
    /// caller reaps.
    fn adopt(child: &mut Child) -> io::Result<Job> {
        let tree = match Job::new() {
            Ok(tree) => tree,
            Err(error) => {
                drop(child.kill());
                return Err(error);
            }
        };
        // SAFETY: `child` owns its process handle and is alive for the call; `tree.job` is the
        // job handle `Tree::new` created and `tree` still holds.
        let joined = unsafe { AssignProcessToJobObject(tree.job, HANDLE(child.as_raw_handle())) };
        if let Err(error) = joined {
            // Still suspended, so it started nothing: ending it ends the tree.
            drop(child.kill());
            return Err(os_error("could not put the command in its job", &error));
        }
        if let Err(error) = resume(child.id()) {
            drop(tree.kill());
            return Err(error);
        }
        Ok(tree)
    }

    /// Ends every process in the job. Windows has no SIGTERM to send a console command that
    /// another process started, so this is the whole stop.
    fn terminate(&self) -> io::Result<()> {
        // SAFETY: `self.job` is a job handle `self` holds.
        unsafe { TerminateJobObject(self.job, TERMINATED) }
            .map_err(|error| os_error("could not terminate the job", &error))
    }

    /// The same as `terminate`.
    fn kill(&self) -> io::Result<()> {
        self.terminate()
    }

    /// Always: `terminate` has ended every process in the job at once. One still holding a pipe
    /// as it ends is what the drain waits for.
    fn empty(&self) -> bool {
        true
    }

    /// Ends what is left of the job. The handle closes when the job drops.
    fn finish(&self) {
        drop(self.terminate());
    }
}

impl Job {
    /// A new job that kills its processes when its last handle closes.
    fn new() -> io::Result<Job> {
        // SAFETY: no security attributes and no name are valid arguments; the handle returned is
        // owned by the `Tree` built from it, which closes it once.
        let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| os_error("could not create a job object", &error))?;
        let tree = Job { job };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
            .map_err(io::Error::other)?;
        // SAFETY: `limits` is a live JOBOBJECT_EXTENDED_LIMIT_INFORMATION of `size` bytes, the
        // struct this information class takes, and `tree.job` is a job handle `tree` holds.
        unsafe {
            SetInformationJobObject(
                tree.job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size,
            )
        }
        .map_err(|error| os_error("could not set the job to kill on close", &error))?;
        Ok(tree)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: `self.job` is a handle this tree owns and closes only here.
        drop(unsafe { CloseHandle(self.job) });
    }
}

/// Resumes every thread of process `pid`, which started suspended. Std gives no thread handle, so
/// the threads are found with a Toolhelp snapshot. Fails when it resumes none.
fn resume(pid: u32) -> io::Result<()> {
    // Before the snapshot opens, so no early return leaves its handle open.
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(size_of::<THREADENTRY32>()).map_err(io::Error::other)?,
        ..THREADENTRY32::default()
    };
    // SAFETY: a thread snapshot of the whole system takes no pointer; its handle is closed below,
    // and nothing between here and there returns early.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|error| os_error("could not list the command's threads", &error))?;
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
            "could not resume the command, which started suspended",
        ));
    }
    Ok(())
}

/// Resumes thread `id`. Returns whether it did.
fn resume_thread(id: u32) -> bool {
    // SAFETY: opening a thread by id takes no pointer; the handle is closed below.
    let Ok(thread) = (unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, id) }) else {
        return false;
    };
    // SAFETY: `thread` is a live handle opened with THREAD_SUSPEND_RESUME.
    let count = unsafe { ResumeThread(thread) };
    // SAFETY: `thread` is a handle this function owns and closes only here.
    drop(unsafe { CloseHandle(thread) });
    count != u32::MAX
}

/// An io error saying `what` failed, with the Win32 error.
fn os_error(what: &str, error: &windows::core::Error) -> io::Error {
    io::Error::other(format!("{what}: {error}"))
}
