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

use windows::Win32::Foundation::{
    DUPLICATE_HANDLE_OPTIONS, DuplicateHandle, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
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

/// The guard's job. Dropping it closes the handle, which ends what is left in it.
#[derive(Debug)]
pub(crate) struct Job {
    job: OwnedHandle,
    /// The descriptors every child starts with (`harden.rs`); none only in this module's tests.
    hardened: Option<Hardened>,
}

impl Job {
    /// Starts `launch` in the job, running: nothing the guard starts is ever outside it
    /// (`job/spawn.rs`).
    pub(crate) fn spawn(&self, launch: &Launch<'_>) -> io::Result<Child> {
        spawn::spawn(Some(self.handle()), self.hardened.as_ref(), launch)
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
        // SAFETY: no security attributes and no name are valid arguments; the handle returned
        // is owned by the `Job` built from it, which closes it once.
        let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
            .map_err(|error| io::Error::other(format!("could not create a job: {error}")))?;
        // SAFETY: `raw` is the open handle CreateJobObjectW just returned, which nothing else
        // owns; the `OwnedHandle` closes it once, on drop.
        let owned = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        let job = Job {
            job: owned,
            hardened,
        };
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
