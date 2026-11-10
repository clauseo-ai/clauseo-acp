//! The Windows half of the run's deadline (`deadline.rs`): the kill-on-close job the process
//! puts itself in, and the end at the deadline, `TerminateProcess` on itself.
#![expect(unsafe_code, reason = "FFI to the Win32 job and process calls")]

use std::ffi::c_void;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
use windows::core::PCWSTR;

use super::MachineError;
use super::deadline::{self, RealClock, Watchdog};
use super::lock::os;
use super::outcome::SetupExit;

/// Arms the deadline for a run of `deadline` from now: the job ([`contain`]), then the
/// watchdog on the real clock, which sets the process's cancel flag and at the deadline ends
/// the process with [`SetupExit::DeadlineReached`]. A job that can't be made is logged and the
/// run goes on: the set-up starts no process, so the job only guards against one it might. A
/// watchdog that can't be started is the error: the deadline couldn't be kept, so the run
/// mustn't start.
pub(super) fn arm(deadline: Duration) -> Result<Watchdog, MachineError> {
    if let Err(error) = contain() {
        tracing::warn!(%error, "the set-up couldn't put itself in a kill-on-close job");
    }
    deadline::start(deadline, RealClock, deadline::cancel, || {
        terminate_self(SetupExit::DeadlineReached.as_code());
    })
    .map_err(|error| MachineError::Other(format!("the deadline's watchdog: {error}")))
}

/// Puts this process in a new job whose last handle closing ends every process in it, with no
/// breakaway allowed. The handle is never closed: it closes as the process ends, and closing
/// it sooner would end the process.
fn contain() -> Result<(), MachineError> {
    // SAFETY: no attributes (an unnamed job no other process can open) and no name.
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }
        .map_err(|error| os("CreateJobObjectW", &error))?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: the job is live; `limits` is the structure the class names, for its size.
    let limited = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast::<c_void>(),
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(0),
        )
    }
    .map_err(|error| os("SetInformationJobObject", &error));
    let assigned = limited.and_then(|()| {
        // SAFETY: the job is live; the pseudo-handle of this process needs no closing.
        unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) }
            .map_err(|error| os("AssignProcessToJobObject", &error))
    });
    if assigned.is_err() {
        close(job);
    }
    assigned
}

/// Closes `job`, which this process isn't in.
fn close(job: HANDLE) {
    // SAFETY: the handle came from CreateJobObjectW and is closed once, here. A failed close
    // leaves nothing to do.
    drop(unsafe { CloseHandle(job) });
}

/// Ends this process at once with exit code `code`; should `TerminateProcess` fail, by exiting.
#[expect(
    clippy::exit,
    reason = "the main thread may be stuck in a call; only ending the process ends it"
)]
fn terminate_self(code: u8) {
    // SAFETY: the pseudo-handle of this process; the call doesn't return when it succeeds.
    if let Err(error) = unsafe { TerminateProcess(GetCurrentProcess(), u32::from(code)) } {
        tracing::error!(%error, "the set-up couldn't end itself at its deadline");
        std::process::exit(i32::from(code));
    }
}
