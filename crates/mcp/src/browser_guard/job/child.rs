//! Windows: a child the guard started in its job (`spawn.rs`), with the part of std's
//! `process::Child` the guard uses: its pid, its pipes, `wait`, `try_wait` and `kill`, each as
//! std does it on Windows (Rust 1.99, `library/std/src/sys/process/windows.rs`). Std's `Child`
//! cannot be built from a process handle, so the guard's code names this type on Windows and
//! std's on Unix (`browser_guard.rs`).
//!
//! The pipes are std's own `io::pipe` ends: `ChildStdin::from(OwnedHandle)` wants a handle
//! opened for overlapped I/O, which `CreatePipe` does not give, while `PipeReader` and
//! `PipeWriter` read and write it synchronously and read a broken pipe as the end.
#![expect(
    unsafe_code,
    reason = "FFI to the process APIs, to wait for and end a child the guard started in its job"
)]

use std::io::{self, PipeReader, PipeWriter};
use std::os::windows::io::{AsRawHandle, OwnedHandle, RawHandle};
use std::os::windows::process::ExitStatusExt;
use std::process::ExitStatus;

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, INFINITE, TerminateProcess, WaitForSingleObject,
};

/// The guard's end of a child's stdin.
pub(crate) type ChildStdin = PipeWriter;
/// The guard's end of a child's stderr.
pub(crate) type ChildStderr = PipeReader;

/// The exit code `kill` gives, as std's.
const KILLED: u32 = 1;

/// A running (or exited, unreaped) child. Dropping it closes its handles; the process runs on
/// until it exits or the job ends.
#[derive(Debug)]
pub(crate) struct Child {
    process: OwnedHandle,
    pid: u32,
    pub(crate) stdin: Option<ChildStdin>,
    pub(crate) stdout: Option<PipeReader>,
    pub(crate) stderr: Option<ChildStderr>,
}

impl Child {
    /// The child `CreateProcessW` just made: its process handle, its pid and the guard's pipe
    /// ends.
    pub(super) fn new(
        process: OwnedHandle,
        pid: u32,
        stdin: Option<ChildStdin>,
        stdout: Option<PipeReader>,
        stderr: Option<ChildStderr>,
    ) -> Child {
        Child {
            process,
            pid,
            stdin,
            stdout,
            stderr,
        }
    }

    pub(crate) fn id(&self) -> u32 {
        self.pid
    }

    /// Ends the child with exit code 1. A child that has exited already is no error.
    pub(crate) fn kill(&mut self) -> io::Result<()> {
        // SAFETY: `self.handle()` is the process handle `self` owns, with every right
        // CreateProcessW grants, open until `self` drops.
        let Err(error) = (unsafe { TerminateProcess(self.handle(), KILLED) }) else {
            return Ok(());
        };
        // Windows refuses to end a process that has ended already: no error then, as std.
        if error.code() == ERROR_ACCESS_DENIED.to_hresult() && self.try_wait().is_ok() {
            return Ok(());
        }
        Err(io::Error::from(error))
    }

    /// Closes the child's stdin, as std's `wait` does, then waits for the child to exit.
    pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
        drop(self.stdin.take());
        // SAFETY: as in `kill`; the wait takes no pointer.
        if unsafe { WaitForSingleObject(self.handle(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        self.status()
    }

    /// The child's exit status if it has exited, without waiting.
    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        // SAFETY: as in `wait`.
        match unsafe { WaitForSingleObject(self.handle(), 0) } {
            WAIT_OBJECT_0 => self.status().map(Some),
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// The exit code of the child, which has exited.
    fn status(&self) -> io::Result<ExitStatus> {
        let mut code = 0_u32;
        // SAFETY: as in `kill`, and `code` is a live u32 the call writes.
        unsafe { GetExitCodeProcess(self.handle(), &raw mut code) }.map_err(io::Error::from)?;
        Ok(ExitStatus::from_raw(code))
    }

    fn handle(&self) -> HANDLE {
        HANDLE(self.process.as_raw_handle())
    }
}

impl AsRawHandle for Child {
    fn as_raw_handle(&self) -> RawHandle {
        self.process.as_raw_handle()
    }
}
