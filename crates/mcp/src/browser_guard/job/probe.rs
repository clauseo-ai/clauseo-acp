//! Windows: whether a job with UI restrictions can be nested where the guard runs, the one thing
//! Chrome's sandbox needs of the jobs above it (decision record 0029). Chromium puts each
//! renderer in a job of its own with UI restrictions, and Windows refuses to nest such a job
//! below any job, however far up, that has UI restrictions itself: `AssignProcessToJobObject`
//! fails with error 50 (`ERROR_NOT_SUPPORTED`), and sandboxed Chrome crashes at its start
//! (`0x80000003`, no child), even inside a job without UI restrictions nested in that one, which
//! is the guard's job's shape. Measured on stage C's Windows VM, with every UI bit on both jobs,
//! and with a UI-limited ancestor above a job without them. `QueryInformationJobObject` with no
//! job reads only the innermost job, so it cannot see such an ancestor.
//!
//! So [`ui_jobs_nest`] tries it: it starts a trivial child suspended (the guard's own program,
//! which never runs a single instruction), makes a throwaway job with one UI restriction
//! (`JOB_OBJECT_UILIMIT_HANDLES`), and assigns the child to it. The child is in every job above
//! the guard, as Chrome is (the guard's own job, between them for Chrome, has no UI restrictions:
//! `job.rs`). The child is always terminated and every handle closed, whatever fails.
#![expect(
    unsafe_code,
    reason = "FFI to CreateProcessW, the Job Object APIs and TerminateProcess, for the probe of \
              whether a UI-restricted job nests where the guard runs"
)]

use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{ERROR_NOT_SUPPORTED, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_UILIMIT_HANDLES,
    JOBOBJECT_BASIC_UI_RESTRICTIONS, JobObjectBasicUIRestrictions, SetInformationJobObject,
};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    TerminateProcess,
};
use windows::core::{PCWSTR, PWSTR};

/// What the probe found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Nesting {
    /// The UI-restricted job took the child: Chrome's sandbox can nest its jobs here.
    Nests,
    /// Error 50: a job above the guard has UI restrictions, so Chrome's sandbox cannot start.
    Refused,
    /// Something else failed (the child or the job could not be made, another error): the probe
    /// cannot tell, and the guard goes on; a Chrome that then crashes at its start is refused
    /// all the same (`chrome/postcheck/crashed.rs`).
    Unknown,
}

/// Whether a job with UI restrictions nests where the guard runs (above).
pub(crate) fn ui_jobs_nest() -> Nesting {
    let Some((process, _thread)) = suspended_child() else {
        return Nesting::Unknown;
    };
    let nesting = assign_to_ui_job(&process);
    // SAFETY: `process` is the live handle of the suspended child CreateProcessW gave, with
    // PROCESS_ALL_ACCESS; terminating a child the guard owns and never resumed is sound. A
    // failure leaves the child to the jobs above, which end with the guard; nothing else to do.
    drop(unsafe { TerminateProcess(HANDLE(process.as_raw_handle()), 1) });
    nesting
}

/// The guard's own program, started suspended with no window, its process and thread handles.
fn suspended_child() -> Option<(OwnedHandle, OwnedHandle)> {
    let program = std::env::current_exe().ok()?;
    let mut wide: Vec<u16> = program.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return None;
    }
    wide.push(0);
    // The command line is the program alone, quoted; CreateProcessW may write to it.
    let mut line: Vec<u16> = std::iter::once(u16::from(b'"'))
        .chain(program.as_os_str().encode_wide())
        .chain([u16::from(b'"'), 0])
        .collect();
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).ok()?,
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: `wide` and `line` are live NUL-terminated UTF-16 strings (`line` writable, as the
    // call requires), `startup` a STARTUPINFOW whose `cb` is set and `info` a live
    // PROCESS_INFORMATION the call fills; no handle is inherited, and no environment or folder
    // is passed (the guard's own are used).
    unsafe {
        CreateProcessW(
            PCWSTR(wide.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_SUSPENDED | CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &raw const startup,
            &raw mut info,
        )
    }
    .ok()?;
    // SAFETY: both are the open handles CreateProcessW just returned, which nothing else owns;
    // each `OwnedHandle` closes its handle once, on drop.
    Some(unsafe {
        (
            OwnedHandle::from_raw_handle(info.hProcess.0),
            OwnedHandle::from_raw_handle(info.hThread.0),
        )
    })
}

/// Makes a job with `JOB_OBJECT_UILIMIT_HANDLES` and assigns `process` to it.
fn assign_to_ui_job(process: &OwnedHandle) -> Nesting {
    // SAFETY: no security attributes and no name are valid arguments; the handle is owned below.
    let Ok(raw) = (unsafe { CreateJobObjectW(None, PCWSTR::null()) }) else {
        return Nesting::Unknown;
    };
    // SAFETY: `raw` is the open handle CreateJobObjectW just returned, which nothing else owns;
    // the `OwnedHandle` closes it once, on drop, after the child is terminated or not.
    let job = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let limits = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: JOB_OBJECT_UILIMIT_HANDLES,
    };
    let Ok(size) = u32::try_from(size_of::<JOBOBJECT_BASIC_UI_RESTRICTIONS>()) else {
        return Nesting::Unknown;
    };
    // SAFETY: `limits` is a live JOBOBJECT_BASIC_UI_RESTRICTIONS of `size` bytes, the struct this
    // class takes, and `job` the job handle owned above.
    let set = unsafe {
        SetInformationJobObject(
            HANDLE(job.as_raw_handle()),
            JobObjectBasicUIRestrictions,
            (&raw const limits).cast(),
            size,
        )
    };
    if set.is_err() {
        return Nesting::Unknown;
    }
    // SAFETY: both are live handles owned here: the job above and the suspended child.
    match unsafe {
        AssignProcessToJobObject(HANDLE(job.as_raw_handle()), HANDLE(process.as_raw_handle()))
    } {
        Ok(()) => Nesting::Nests,
        Err(error) if error.code() == ERROR_NOT_SUPPORTED.to_hresult() => Nesting::Refused,
        Err(_) => Nesting::Unknown,
    }
}
