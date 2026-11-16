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
//! Each end ([`Job::kill`], the browser's stop in `session.rs`) waits until no process is left
//! in the job, and then puts a new, empty job in its place for the next start: Windows creates no
//! process in a job that is terminating (`AssignProcessToJobObject`'s remarks), so a start in the
//! ended job failed, and `TerminateJobObject` returns before the processes are gone, whose files
//! (the profile, the executables) stay open until they are.
//!
//! The guard's hardened copy (`harden.rs`) is the one process started outside such a job
//! ([`start_copy`]): it stays in the guard's own jobs, as any child does, and makes its own job
//! for Chrome and agent-browser. Its first process waits for it, and the copy watches that
//! process as its parent (`parent.rs`), so whichever way the first process ends, the copy ends
//! the browser and cleans up; a job that ended the copy with it would skip that cleanup.
//!
//! `clauseo-process` makes its job the same way, but for its async child and with dependencies
//! this crate may not take (`ARCHITECTURE.md`), so the few job calls are repeated here.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 Job Object APIs, for agent-browser's tree"
)]

mod child;
mod spawn;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use windows::Win32::Foundation::{
    DUPLICATE_HANDLE_OPTIONS, DuplicateHandle, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, INFINITE, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::core::PCWSTR;

use super::harden::Hardened;

pub(crate) use child::{Child, ChildStderr, ChildStdin};
pub(crate) use spawn::{Io, Launch, inheritable, windows_dirs};

/// The exit code the job's end gives its processes.
const TERMINATED: u32 = 1;
/// How long [`Job::kill`] waits for the ended job's processes to be gone, as the Unix stop waits
/// for Chrome's group (`chrome.rs`).
const END_WAIT: Duration = Duration::from_secs(5);
/// How often that wait looks.
const END_LOOK: Duration = Duration::from_millis(10);

/// The guard's job. Dropping it closes the handle, which ends what is left in it.
#[derive(Debug)]
pub(crate) struct Job {
    /// The job's handle: a new job's after each [`Job::kill`]. Held locked while a child is
    /// created in it, so an end never swaps it in the middle of a creation.
    job: Mutex<OwnedHandle>,
    /// The descriptors every child starts with (`harden.rs`); none only in this module's tests.
    hardened: Option<Hardened>,
}

impl Job {
    /// Starts `launch` in the job, running: nothing the guard starts is ever outside it
    /// (`job/spawn.rs`).
    pub(crate) fn spawn(&self, launch: &Launch<'_>) -> io::Result<Child> {
        let job = self.lock();
        spawn::spawn(
            Some(HANDLE(job.as_raw_handle())),
            self.hardened.as_ref(),
            launch,
        )
    }

    /// Ends every process in the job (Chrome, agent-browser and its daemon), waits until none
    /// is left in it, at most [`END_WAIT`], and puts a new, empty job in its place, which the
    /// next start uses. Returns whether Windows took the end.
    ///
    /// `TerminateJobObject` returns before the processes are gone; until they are, their files
    /// stay open (the profile, and the executables, which a test's folder holds). And Windows
    /// creates no process in a job that is terminating (`AssignProcessToJobObject`'s remarks;
    /// `PROC_THREAD_ATTRIBUTE_JOB_LIST` then fails the creation, `job/spawn.rs`): the start after
    /// a stop, in the ended job, failed with "the browser could not start" (Windows CI, PR
    /// #1140). The old handle closes once the new job is in place, which ends anything still in
    /// the old one (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`).
    pub(crate) fn kill(&self) -> bool {
        let mut job = self.lock();
        let handle = HANDLE(job.as_raw_handle());
        // SAFETY: `handle` is the job handle `self` owns, open while `job` is locked.
        let ended = unsafe { TerminateJobObject(handle, TERMINATED) }.is_ok();
        if ended && !wait_empty(handle) {
            super::note(
                "processes in the guard's job were still running when the guard stopped waiting \
                 for them",
            );
        }
        match make_job() {
            Ok(fresh) => *job = fresh,
            Err(error) => super::note(&format!(
                "could not make a new job for the browser's next start: {error}"
            )),
        }
        ended
    }

    /// The job's handle now; a later [`Job::kill`] closes it.
    #[cfg(test)]
    fn handle(&self) -> HANDLE {
        HANDLE(self.lock().as_raw_handle())
    }

    fn lock(&self) -> MutexGuard<'_, OwnedHandle> {
        self.job.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new job that ends its processes when its last handle closes, whose children all
    /// start with `hardened`'s descriptors. No breakaway flag is set, so a process in it cannot
    /// start one outside it.
    pub(crate) fn new(hardened: Hardened) -> io::Result<Job> {
        Self::make(Some(hardened))
    }

    /// A job whose children start unhardened, for this module's tests.
    #[cfg(test)]
    pub(crate) fn unhardened() -> io::Result<Job> {
        Self::make(None)
    }

    fn make(hardened: Option<Hardened>) -> io::Result<Job> {
        Ok(Job {
            job: Mutex::new(make_job()?),
            hardened,
        })
    }
}

/// A new job that ends its processes when its last handle closes; no breakaway flag is set.
fn make_job() -> io::Result<OwnedHandle> {
    // SAFETY: no security attributes and no name are valid arguments; the handle returned is
    // owned by the `OwnedHandle` built from it, which closes it once.
    let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|error| io::Error::other(format!("could not create a job: {error}")))?;
    // SAFETY: `raw` is the open handle CreateJobObjectW just returned, which nothing else owns;
    // the `OwnedHandle` closes it once, on drop (also when the limit below fails).
    let owned = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let size = u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
        .map_err(io::Error::other)?;
    // SAFETY: `limits` is a live JOBOBJECT_EXTENDED_LIMIT_INFORMATION of `size` bytes, the
    // struct this information class takes, and the handle is the one `owned` holds open.
    unsafe {
        SetInformationJobObject(
            HANDLE(owned.as_raw_handle()),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size,
        )
    }
    .map_err(|error| io::Error::other(format!("could not set the job's limit: {error}")))?;
    Ok(owned)
}

/// Waits until no process is left in `job` (child jobs counted), at most [`END_WAIT`]; whether
/// none is. A job whose count cannot be read is not waited for.
fn wait_empty(job: HANDLE) -> bool {
    for _ in 0..END_WAIT.as_millis() / END_LOOK.as_millis() {
        match active_processes(job) {
            Some(0) => return true,
            Some(_) => super::pause(END_LOOK),
            None => return false,
        }
    }
    active_processes(job) == Some(0)
}

/// How many processes are in `job` now, child jobs included; `None` if Windows will not say.
fn active_processes(job: HANDLE) -> Option<u32> {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    let size = u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>()).ok()?;
    // SAFETY: `info` is a live JOBOBJECT_BASIC_ACCOUNTING_INFORMATION of `size` bytes, the
    // struct this information class fills, and `job` is open for the call (its owner holds the
    // lock); no length is asked back.
    unsafe {
        QueryInformationJobObject(
            Some(job),
            JobObjectBasicAccountingInformation,
            (&raw mut info).cast(),
            size,
            None,
        )
    }
    .ok()?;
    Some(info.ActiveProcesses)
}

/// The guard's hardened copy, as its first process holds it: by a handle that may only wait for
/// it and read its exit code.
#[derive(Debug)]
pub(crate) struct HardenedCopy(OwnedHandle);

impl HardenedCopy {
    /// Waits for the copy to exit and returns its exit code.
    pub(crate) fn wait(&self) -> io::Result<u32> {
        let handle = HANDLE(self.0.as_raw_handle());
        // SAFETY: `handle` is open with SYNCHRONIZE while `self` lives; the wait takes no pointer.
        if unsafe { WaitForSingleObject(handle, INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0_u32;
        // SAFETY: `handle` is open with PROCESS_QUERY_LIMITED_INFORMATION; `code` is a live u32
        // the call writes.
        unsafe { GetExitCodeProcess(handle, &raw mut code) }.map_err(io::Error::from)?;
        Ok(code)
    }
}

/// Starts the guard's hardened copy (`harden.rs`): this program again, as
/// `browser-guard --hardened-copy <args...>`, with this process's environment, folder and
/// standard handles, and `hardened`'s descriptors from its creation. Keeps only a handle that
/// may wait for it and read its exit code: the all-access one `CreateProcessW` returns is
/// closed at once, so no process that could take a handle of this one gets that one.
pub(crate) fn start_copy(hardened: &Hardened, args: &[OsString]) -> io::Result<HardenedCopy> {
    let program = std::env::current_exe()?;
    let cwd = std::env::current_dir()?;
    let mut env = BTreeMap::new();
    for (name, value) in std::env::vars_os() {
        let name = name.into_string().map_err(|name| {
            io::Error::other(format!(
                "the environment variable name {} is not Unicode",
                name.display()
            ))
        })?;
        env.insert(name, value);
    }
    let copy_args: Vec<OsString> = ["browser-guard", super::harden::HARDENED_COPY]
        .into_iter()
        .map(OsString::from)
        .chain(args.iter().cloned())
        .collect();
    let mut child = spawn::spawn(
        None,
        Some(hardened),
        &Launch {
            program: &program,
            args: &copy_args,
            cwd: &cwd,
            env: &env,
            stdin: Io::Inherit,
            stdout: Io::Inherit,
            stderr: Io::Inherit,
            inherit: &[],
        },
    )?;
    let mut limited = HANDLE::default();
    // SAFETY: the current process's pseudo handle needs no closing; `child`'s handle is open
    // for the call, and `limited` is a live HANDLE the call writes. Not inheritable.
    let duplicated = unsafe {
        let me = GetCurrentProcess();
        DuplicateHandle(
            me,
            HANDLE(child.as_raw_handle()),
            me,
            &raw mut limited,
            (PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION).0,
            false,
            DUPLICATE_HANDLE_OPTIONS(0),
        )
    };
    if let Err(error) = duplicated {
        // A copy this process cannot wait for must not run on.
        drop(child.kill());
        return Err(io::Error::from(error));
    }
    // SAFETY: `limited` is the open handle DuplicateHandle just made, which nothing else owns.
    let copy = HardenedCopy(unsafe { OwnedHandle::from_raw_handle(limited.0) });
    drop(child);
    Ok(copy)
}

#[cfg(test)]
mod harden_tests;
#[cfg(test)]
mod tests;
