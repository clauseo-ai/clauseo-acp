//! The job's completion port on Windows: its association with the job and its reader thread.
#![expect(
    unsafe_code,
    reason = "FFI to the completion port and the job's association"
)]

use std::ffi::c_void;
use std::io;
use std::ptr;
use std::thread;

use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows::Win32::System::IO::{CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED};
use windows::Win32::System::JobObjects::{
    JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JobObjectAssociateCompletionPortInformation,
    SetInformationJobObject,
};
use windows::Win32::System::SystemServices::JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT;
use windows::Win32::System::Threading::INFINITE;

use super::{ACTIVE_PROCESS_LIMIT, watch};
use crate::windows::acl::object::{Handle, os_error};

const _: () = assert!(ACTIVE_PROCESS_LIMIT == JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT);

/// The port, owned by its reader thread, which lives until this process exits.
struct Port(Handle);

// SAFETY: a completion port is a kernel handle any thread may use; only the reader holds it.
unsafe impl Send for Port {}

impl Iterator for Port {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        let (mut message, mut key) = (0u32, 0usize);
        let mut overlapped: *mut OVERLAPPED = ptr::null_mut();
        // SAFETY: the port is live; the three outputs are writable locals. A job message
        // carries its id in the byte count and the process id in `overlapped`.
        unsafe {
            GetQueuedCompletionStatus(
                self.0.0,
                &raw mut message,
                &raw mut key,
                &raw mut overlapped,
                INFINITE,
            )
        }
        .ok()
        .map(|()| message)
    }
}

/// Gives `job` a completion port and starts its reader, which calls `notify` as [`watch`]
/// says. A failure leaves the cap in force with no notice.
pub(in super::super) fn watch_job(job: &Handle, notify: fn(&str)) -> io::Result<()> {
    // SAFETY: a new port, tied to no file.
    let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1) }
        .map_err(|e| os_error(&e))?;
    let port = Port(Handle(port));
    let info = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
        CompletionKey: ptr::null_mut(),
        CompletionPort: port.0.0,
    };
    // SAFETY: the job is live; `info` is the structure the class names, for its size.
    unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectAssociateCompletionPortInformation,
            (&raw const info).cast::<c_void>(),
            u32::try_from(size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>()).unwrap_or(0),
        )
    }
    .map_err(|e| os_error(&e))?;
    thread::Builder::new().spawn(move || watch(port, notify))?;
    Ok(())
}
