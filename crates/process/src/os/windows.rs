//! The Windows half: the agent runs in a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
//! which this process owns, so Windows ends the agent's tree when Clauseo dies, however it dies.
//! The agent starts suspended, joins the job, and only then runs, so nothing it starts can run
//! outside the job. It starts with `CREATE_NO_WINDOW`, so it has no console, and so no Ctrl-Break
//! to receive: Windows has no SIGTERM step. Every Win32 call of the crate is in this file.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 Job Object, process and thread APIs"
)]

use std::io;
use std::os::windows::process::CommandExt;
use std::process::Command;

use async_process::Child;
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
    CREATE_NO_WINDOW, CREATE_SUSPENDED, OpenProcess, OpenThread, PROCESS_SET_QUOTA,
    PROCESS_TERMINATE, ResumeThread, THREAD_SUSPEND_RESUME,
};
use windows::core::PCWSTR;

use crate::{Error, SpawnOptions};

/// The exit code `TerminateJobObject` gives the processes it ends.
const TERMINATED: u32 = 1;

/// Makes the agent start suspended and without a console, plus any flags `opts` adds.
pub(crate) fn prepare(command: &mut Command, opts: &SpawnOptions) {
    command.creation_flags(CREATE_SUSPENDED.0 | CREATE_NO_WINDOW.0 | opts.creation_flags);
}

/// The agent's Job Object. Dropping it closes the handle, which ends what is left of the tree.
#[derive(Debug)]
pub(crate) struct Tree {
    job: HANDLE,
}

// SAFETY: a job handle is a kernel handle that any thread may use; only `Drop` closes it.
unsafe impl Send for Tree {}

impl Tree {
    /// The tree of `child`, just started suspended from a `prepare`d command: a new job holds it,
    /// then its threads resume. When a step fails, the child is ended and the error returned; an
    /// agent never runs outside its job.
    pub(crate) fn adopt(child: &mut Child) -> Result<Tree, Error> {
        let tree = match Tree::new() {
            Ok(tree) => tree,
            Err(error) => {
                drop(child.kill());
                return Err(error);
            }
        };
        if let Err(error) = join(&tree, child.id()) {
            // Still suspended, so it started nothing: ending it ends the tree.
            drop(child.kill());
            return Err(error);
        }
        if let Err(error) = resume(child.id()) {
            tree.kill();
            return Err(error);
        }
        Ok(tree)
    }

    /// Ends every process in the job.
    pub(crate) fn kill(&self) {
        // SAFETY: `self.job` is a job handle `self` holds.
        drop(unsafe { TerminateJobObject(self.job, TERMINATED) });
    }

    /// A new job that ends its processes when its last handle closes.
    fn new() -> Result<Tree, Error> {
        // SAFETY: no security attributes and no name are valid arguments; the handle returned is
        // owned by the `Tree` built from it, which closes it once.
        let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| os_error("could not create a job object", error))?;
        let tree = Tree { job };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let size =
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).map_err(|source| {
                Error::Os {
                    what: "the job limit struct is too large",
                    source: io::Error::other(source),
                }
            })?;
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
        .map_err(|error| os_error("could not set the job to kill on close", error))?;
        Ok(tree)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        // SAFETY: `self.job` is a handle this tree owns and closes only here.
        drop(unsafe { CloseHandle(self.job) });
    }
}

/// Puts process `pid` in `tree`'s job. async-process gives no process handle, so one is opened by
/// pid: the caller's `Child` holds its own handle, so the pid cannot be reused meanwhile.
fn join(tree: &Tree, pid: u32) -> Result<(), Error> {
    // SAFETY: opening a process by id takes no pointer; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) }
        .map_err(|error| os_error("could not open the agent's process", error))?;
    // SAFETY: `tree.job` is the job handle `tree` holds, and `process` the live handle opened
    // above with the rights AssignProcessToJobObject needs.
    let joined = unsafe { AssignProcessToJobObject(tree.job, process) };
    // SAFETY: `process` is a handle this function owns and closes only here.
    drop(unsafe { CloseHandle(process) });
    joined.map_err(|error| os_error("could not put the agent in its job", error))
}

/// Resumes every thread of process `pid`, which started suspended. Std gives no thread handle, so
/// the threads are found with a Toolhelp snapshot. Fails when it resumes none.
fn resume(pid: u32) -> Result<(), Error> {
    // Before the snapshot opens, so no early return leaves its handle open.
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(size_of::<THREADENTRY32>()).map_err(|source| Error::Os {
            what: "the thread entry struct is too large",
            source: io::Error::other(source),
        })?,
        ..THREADENTRY32::default()
    };
    // SAFETY: a thread snapshot of the whole system takes no pointer; its handle is closed below,
    // and nothing between here and there returns early.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
        .map_err(|error| os_error("could not list the agent's threads", error))?;
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
        return Err(Error::Os {
            what: "could not resume the agent, which started suspended",
            source: io::Error::other("no thread of the agent resumed"),
        });
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

/// An [`Error::Os`] saying `what` failed, with the Win32 error.
fn os_error(what: &'static str, error: windows::core::Error) -> Error {
    Error::Os {
        what,
        source: io::Error::from(error),
    }
}
