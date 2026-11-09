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

use super::{ACTIVE_PROCESS_LIMIT, hand_off, watch};
use crate::windows::acl::object::{Handle, os_error};

const _: () = assert!(ACTIVE_PROCESS_LIMIT == JOB_OBJECT_MSG_ACTIVE_PROCESS_LIMIT);

/// The job's port; the outer's reader thread owns it until this process exits. Each item is a
/// `JOB_OBJECT_MSG_*`; it ends when a wait of `wait` ms passes with none, or the port fails.
pub(in super::super) struct Port {
    handle: Handle,
    wait: u32,
}

impl Port {
    /// The same port, its waits cut to `wait` ms.
    #[cfg(test)]
    pub(in super::super) fn waiting(self, wait: u32) -> Port {
        Port { wait, ..self }
    }
}

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
                self.handle.0,
                &raw mut message,
                &raw mut key,
                &raw mut overlapped,
                self.wait,
            )
        }
        .ok()
        .map(|()| message)
    }
}

/// Gives `job` a new completion port: the job posts its messages to it from now on.
pub(in super::super) fn associate(job: &Handle) -> io::Result<Port> {
    // SAFETY: a new port, tied to no file.
    let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, None, 0, 1) }
        .map_err(|e| os_error(&e))?;
    let port = Port {
        handle: Handle(port),
        wait: INFINITE,
    };
    let info = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
        CompletionKey: ptr::null_mut(),
        CompletionPort: port.handle.0,
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
    Ok(port)
}

/// Gives `job` a completion port and starts its reader, which tells `say` as [`watch`] says,
/// through [`hand_off`]. A failure leaves the cap in force with no notice.
pub(in super::super) fn watch_job(job: &Handle, say: fn(&str)) -> io::Result<()> {
    let port = associate(job)?;
    let notify = hand_off(say)?;
    thread::Builder::new().spawn(move || watch(port, notify))?;
    Ok(())
}
