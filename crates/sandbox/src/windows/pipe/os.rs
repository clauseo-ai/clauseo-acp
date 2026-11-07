//! The runner pipes on Windows: the outer runner's server ends, made with the explicit DACL
//! and overlapped so a wait can also watch the inner runner's process, and the inner runner's
//! client ends, opened plainly (synchronous) so the agent can use them as its stdio.
#![expect(unsafe_code, reason = "FFI to the named pipe and overlapped I/O calls")]

use std::io;
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_PIPE_CONNECTED,
    ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES,
    FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, PIPE_ACCESS_INBOUND, PIPE_ACCESS_OUTBOUND,
    ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects};
use windows::core::HSTRING;

use super::Role;
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::token::os::LocalDescriptor;

/// The pipes' buffer size each way.
const BUFFER: u32 = 64 * 1024;

/// A server end the outer runner made, opened for overlapped I/O.
pub(in crate::windows) struct ServerPipe {
    handle: Handle,
    role: Role,
}

impl ServerPipe {
    /// Makes `name`, `role`'s pipe, with the descriptor `sddl`: one instance, made first or
    /// refused (`FILE_FLAG_FIRST_PIPE_INSTANCE`), local clients only.
    pub(in crate::windows) fn create(name: &str, role: Role, sddl: &str) -> io::Result<ServerPipe> {
        let descriptor = LocalDescriptor::new(sddl)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: false.into(),
        };
        let access = match role {
            Role::Ctl => PIPE_ACCESS_DUPLEX,
            Role::In => PIPE_ACCESS_OUTBOUND,
            Role::Out | Role::Err => PIPE_ACCESS_INBOUND,
        };
        // SAFETY: the name is a live HSTRING; `attributes` and the descriptor it points at
        // outlive the call; the handle returned is owned by `Handle`.
        let handle = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(name),
                access | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                BUFFER,
                BUFFER,
                0,
                Some(&raw const attributes),
            )
        };
        if handle.is_invalid() {
            return Err(io::Error::last_os_error());
        }
        Ok(ServerPipe {
            handle: Handle(handle),
            role,
        })
    }

    /// Which pipe this is.
    pub(in crate::windows) fn role(&self) -> Role {
        self.role
    }

    /// Waits up to `timeout` for a client, giving up early when `watch` (the inner runner's
    /// process) is signalled: it exited before connecting.
    pub(in crate::windows) fn connect(&self, watch: HANDLE, timeout: Duration) -> io::Result<()> {
        let mut io = Overlapped::new()?;
        // SAFETY: the pipe is a live server end; `io` outlives the operation, which is waited
        // for or cancelled below before `io` drops.
        match unsafe { ConnectNamedPipe(self.handle.0, Some(io.as_mut_ptr())) } {
            Ok(()) => return Ok(()),
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => return Ok(()),
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {}
            Err(error) => return Err(os_error(&error)),
        }
        let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: both handles are live for the wait.
        let woke = unsafe { WaitForMultipleObjects(&[io.event.0, watch], false, millis) };
        if woke == WAIT_OBJECT_0 {
            return io.result(&self.handle).map(drop);
        }
        io.cancel(&self.handle);
        Err(if woke == WAIT_TIMEOUT {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "the inner runner didn't connect in time",
            )
        } else {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "the inner runner exited before connecting",
            )
        })
    }

    /// The pid of the client connected now.
    pub(in crate::windows) fn client_pid(&self) -> io::Result<u32> {
        let mut pid = 0u32;
        // SAFETY: the pipe is a live server end; `pid` is a live out-pointer.
        unsafe { GetNamedPipeClientProcessId(self.handle.0, &raw mut pid) }
            .map_err(|error| os_error(&error))?;
        Ok(pid)
    }
}

// SAFETY: the handle is a kernel handle any thread may use, and every operation on it here is
// overlapped with its own OVERLAPPED and event, so the relay's threads may share one pipe.
unsafe impl Send for ServerPipe {}
// SAFETY: as for Send: no operation shares state through `&self` but the kernel handle.
unsafe impl Sync for ServerPipe {}

impl io::Read for &ServerPipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut io = Overlapped::new()?;
        let len = buf.len().min(BUFFER as usize);
        // SAFETY: `buf` and `io` outlive the operation, which is completed by `result` (which
        // waits) before either drops.
        let started = unsafe {
            ReadFile(
                self.handle.0,
                buf.get_mut(..len),
                None,
                Some(io.as_mut_ptr()),
            )
        };
        match started {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {}
            Err(error) if ended(&error) => return Ok(0),
            Err(error) => return Err(os_error(&error)),
        }
        match io.result(&self.handle) {
            Ok(read) => Ok(read),
            Err(error) if error.raw_os_error().is_some_and(ended_code) => Ok(0),
            Err(error) => Err(error),
        }
    }
}

impl io::Write for &ServerPipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut io = Overlapped::new()?;
        let len = buf.len().min(BUFFER as usize);
        // SAFETY: `buf` and `io` outlive the operation, which `result` waits for.
        let started =
            unsafe { WriteFile(self.handle.0, buf.get(..len), None, Some(io.as_mut_ptr())) };
        match started {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {}
            Err(error) => return Err(os_error(&error)),
        }
        io.result(&self.handle)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Whether `error` means the other end is gone: the end of the stream.
fn ended(error: &windows::core::Error) -> bool {
    [ERROR_BROKEN_PIPE, ERROR_PIPE_NOT_CONNECTED, ERROR_NO_DATA]
        .iter()
        .any(|code| error.code() == code.to_hresult())
}

/// [`ended`] for a raw OS code.
fn ended_code(code: i32) -> bool {
    [ERROR_BROKEN_PIPE, ERROR_PIPE_NOT_CONNECTED, ERROR_NO_DATA]
        .iter()
        .any(|known| i32::try_from(known.0).is_ok_and(|known| known == code))
}

/// One overlapped operation's state and its manual-reset event.
struct Overlapped {
    overlapped: Box<OVERLAPPED>,
    event: Handle,
}

impl Overlapped {
    fn new() -> io::Result<Overlapped> {
        // SAFETY: no attributes and no name; the event is owned by `Handle`.
        let event = unsafe { CreateEventW(None, true, false, None) }.map_err(|e| os_error(&e))?;
        let event = Handle(event);
        let overlapped = Box::new(OVERLAPPED {
            hEvent: event.0,
            ..OVERLAPPED::default()
        });
        Ok(Overlapped { overlapped, event })
    }

    fn as_mut_ptr(&mut self) -> *mut OVERLAPPED {
        &raw mut *self.overlapped
    }

    /// Waits for the operation on `handle` and gives the bytes it moved.
    fn result(&self, handle: &Handle) -> io::Result<usize> {
        let mut moved = 0u32;
        // SAFETY: the operation was started with this OVERLAPPED on `handle`; waiting is
        // allowed, and `moved` is a live out-pointer.
        unsafe { GetOverlappedResult(handle.0, &raw const *self.overlapped, &raw mut moved, true) }
            .map_err(|error| os_error(&error))?;
        usize::try_from(moved).map_err(io::Error::other)
    }

    /// Cancels the operation on `handle` and waits for it to finish, so nothing writes into
    /// this state after it drops.
    fn cancel(&self, handle: &Handle) {
        // SAFETY: the operation was started with this OVERLAPPED on `handle`.
        drop(unsafe { CancelIoEx(handle.0, Some(&raw const *self.overlapped)) });
        drop(self.result(handle));
    }
}

/// Opens the client end of `name`, `role`'s pipe, for the inner runner: synchronous, so the
/// agent can use it as a standard handle; inheritable when `inherit` (the agent's three); with
/// identification-only quality of service, so the server can learn the client's identity but
/// never act as it.
pub(in crate::windows) fn open_client(name: &str, role: Role, inherit: bool) -> io::Result<Handle> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: inherit.into(),
    };
    let access = match role {
        Role::Ctl => GENERIC_READ.0 | GENERIC_WRITE.0,
        Role::In => GENERIC_READ.0,
        Role::Out | Role::Err => GENERIC_WRITE.0,
    };
    // SAFETY: the name is a live HSTRING and `attributes` outlives the call; the handle
    // returned is owned by `Handle`.
    let handle = unsafe {
        CreateFileW(
            &HSTRING::from(name),
            access,
            FILE_SHARE_NONE,
            Some(&raw const attributes),
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0),
            None,
        )
    }
    .map_err(|error| os_error(&error))?;
    Ok(Handle(handle))
}
