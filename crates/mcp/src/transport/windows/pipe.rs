//! Windows only: one end of the named pipe as a [`Connection`].
//!
//! The handle is opened for overlapped I/O, and every read and write is one overlapped
//! operation with its own event that the calling thread waits on to the end. A synchronous pipe
//! handle would serialise them: a read blocked on one thread would hold back a write on another,
//! and the relay reads and writes a connection from two threads at once. A clone shares the
//! handle ([`Arc`]), as a socket's clones share the socket.
//!
//! Each wait also watches the connection's shut event. [`Connection::shutdown`] with
//! [`Shutdown::Both`] sets it and cancels whatever is pending, so a read or a write blocked on
//! another clone ends (a read as the end of input, a write with an error); on the guard's end it
//! also disconnects the client. A pipe has no half-close: `Read` and `Write` alone do nothing,
//! and the peer sees the end only when the last clone closes. A read or write timeout
//! ([`Connection::set_read_timeout`], [`Connection::set_write_timeout`]) cancels an operation
//! still pending when it passes, which then fails with `TimedOut`.
//!
//! The peer check ([`Connection::peer_is`]) depends on the end: the guard's asks whether the
//! client runs as the `--client-sid` user, in the `--client-package-sid` package with no
//! capability when that is given (`sid::client_is`); the bridge's whether the pipe is owned by
//! the `--server-sid` user (`sid::owner_of`) and its server is the guard's process, by pid and
//! creation time (`identity::server_is`).
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 overlapped pipe I/O, for the named pipe transport"
)]

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED, ERROR_PIPE_CONNECTED,
    ERROR_PIPE_NOT_CONNECTED, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{ConnectNamedPipe, DisconnectNamedPipe};
use windows::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
};
use windows::core::PCWSTR;

use super::identity;
use super::sid::{self, Sid};
use crate::transport::{Connection, Peer};

/// The most bytes one read or write moves.
const MAX_IO: usize = 1 << 20;

/// Which end of the pipe, with what its peer check expects.
pub(super) enum End {
    /// The guard's: the client must run as this user, and in this `AppContainer` package with no
    /// capability when one is given.
    Server { client: Sid, package: Option<Sid> },
    /// The bridge's: the pipe must be owned by this user.
    Client { server: Sid },
}

/// One end of a connected pipe; clones share it.
pub(crate) struct Pipe {
    inner: Arc<Inner>,
}

struct Inner {
    handle: OwnedHandle,
    /// Set once by [`Shutdown::Both`]; every wait watches it.
    shut: OwnedHandle,
    end: End,
    write_timeout: Mutex<Option<Duration>>,
    read_timeout: Mutex<Option<Duration>>,
}

/// Why an operation ended early.
enum Cut {
    Shut,
    TimedOut,
}

impl Pipe {
    /// Wraps `handle`, opened with `FILE_FLAG_OVERLAPPED`.
    pub(super) fn new(handle: OwnedHandle, end: End) -> io::Result<Pipe> {
        Ok(Pipe {
            inner: Arc::new(Inner {
                handle,
                shut: event()?,
                end,
                write_timeout: Mutex::new(None),
                read_timeout: Mutex::new(None),
            }),
        })
    }

    fn raw(&self) -> HANDLE {
        HANDLE(self.inner.handle.as_raw_handle())
    }

    /// The handle, for the tests.
    #[cfg(test)]
    pub(super) fn handle(&self) -> &OwnedHandle {
        &self.inner.handle
    }

    /// Server end: waits until a client connects.
    pub(super) fn connect(&self) -> io::Result<()> {
        // SAFETY: the handle and the OVERLAPPED are live until `overlapped` has waited the
        // operation out.
        let started = self.overlapped(None, |handle, overlapped| unsafe {
            ConnectNamedPipe(handle, Some(overlapped))
        });
        match started {
            // A client that connected between the pipe's creation and this call.
            Err(error) if error.raw_os_error() == Some(code(ERROR_PIPE_CONNECTED)) => Ok(()),
            other => other.map(drop),
        }
    }

    /// Server end: drops the connected client, if any, so the pipe can take the next.
    pub(super) fn disconnect(&self) {
        // SAFETY: the handle is the open server end `self` owns.
        drop(unsafe { DisconnectNamedPipe(self.raw()) });
    }

    /// Runs one overlapped operation and waits for its end; the bytes it moved. `start` begins
    /// it on the handle with the OVERLAPPED given, which stays valid until it has ended. Ended
    /// early, when `timeout` passes or the connection is shut, it is cancelled and still waited
    /// out, so nothing it points to is freed while Windows may write to it.
    fn overlapped(
        &self,
        timeout: Option<Duration>,
        start: impl FnOnce(HANDLE, *mut OVERLAPPED) -> windows::core::Result<()>,
    ) -> io::Result<u32> {
        let done = event()?;
        let mut overlapped = OVERLAPPED {
            hEvent: HANDLE(done.as_raw_handle()),
            ..OVERLAPPED::default()
        };
        let handle = self.raw();
        let mut cut = None;
        match start(handle, &raw mut overlapped) {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
                let waits = [
                    HANDLE(done.as_raw_handle()),
                    HANDLE(self.inner.shut.as_raw_handle()),
                ];
                let millis = timeout.map_or(INFINITE, |timeout| {
                    u32::try_from(timeout.as_millis())
                        .map_or(INFINITE - 1, |ms| ms.min(INFINITE - 1))
                });
                // SAFETY: both handles are open events owned for the length of the call.
                let woke = unsafe { WaitForMultipleObjects(&waits, false, millis) };
                if woke != WAIT_OBJECT_0 {
                    cut = Some(if self.is_shut() {
                        Cut::Shut
                    } else {
                        Cut::TimedOut
                    });
                    // SAFETY: cancels only this operation, whose OVERLAPPED is live; one that
                    // has just ended makes it fail harmlessly.
                    drop(unsafe { CancelIoEx(handle, Some(&raw const overlapped)) });
                }
            }
            Err(error) => return Err(io_error(&error)),
        }
        let mut moved = 0u32;
        // SAFETY: the OVERLAPPED is the started operation's; bWait waits for its end, after
        // which nothing refers to it or its buffer.
        let ended =
            unsafe { GetOverlappedResult(handle, &raw const overlapped, &raw mut moved, true) };
        match (ended, cut) {
            (Ok(()), _) => Ok(moved),
            (Err(error), Some(cut)) if error.code() == ERROR_OPERATION_ABORTED.to_hresult() => {
                Err(match cut {
                    Cut::Shut => io::ErrorKind::NotConnected.into(),
                    Cut::TimedOut => io::ErrorKind::TimedOut.into(),
                })
            }
            (Err(error), _) => Err(io_error(&error)),
        }
    }

    fn is_shut(&self) -> bool {
        // SAFETY: the shut event is open while `self` lives; a zero wait only polls it.
        let woke = unsafe { WaitForSingleObject(HANDLE(self.inner.shut.as_raw_handle()), 0) };
        woke == WAIT_OBJECT_0
    }
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let len = buf.len().min(MAX_IO);
        let buf = &mut buf[..len];
        let timeout = *self
            .inner
            .read_timeout
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // SAFETY: `buf` and the OVERLAPPED stay borrowed until `overlapped` has waited the read
        // out; the handle is open while `self` lives.
        let read = self.overlapped(timeout, |handle, overlapped| unsafe {
            ReadFile(handle, Some(buf), None, Some(overlapped))
        });
        match read {
            Ok(moved) => usize::try_from(moved).map_err(io::Error::other),
            // The peer closed or disconnected, or this end was shut: the input has ended.
            Err(error)
                if error.kind() == io::ErrorKind::NotConnected
                    || [ERROR_BROKEN_PIPE, ERROR_PIPE_NOT_CONNECTED]
                        .iter()
                        .any(|ended| error.raw_os_error() == Some(code(*ended))) =>
            {
                Ok(0)
            }
            Err(error) => Err(error),
        }
    }
}

impl Write for Pipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let buf = &buf[..buf.len().min(MAX_IO)];
        let timeout = *self
            .inner
            .write_timeout
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // SAFETY: `buf` and the OVERLAPPED stay borrowed until `overlapped` has waited the write
        // out; the handle is open while `self` lives.
        let written = self.overlapped(timeout, |handle, overlapped| unsafe {
            WriteFile(handle, Some(buf), None, Some(overlapped))
        })?;
        match usize::try_from(written).map_err(io::Error::other)? {
            0 => Err(io::ErrorKind::WriteZero.into()),
            written => Ok(written),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Connection for Pipe {
    fn try_clone(&self) -> io::Result<Self> {
        Ok(Pipe {
            inner: Arc::clone(&self.inner),
        })
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        if how != Shutdown::Both {
            // No half-close on a pipe.
            return Ok(());
        }
        // SAFETY: the shut event is open while `self` lives.
        unsafe { SetEvent(HANDLE(self.inner.shut.as_raw_handle())) }?;
        // SAFETY: cancels every operation pending on the open handle, from any thread; each is
        // still waited out by its own `overlapped` call. None pending makes it fail harmlessly.
        drop(unsafe { CancelIoEx(self.raw(), None) });
        if matches!(self.inner.end, End::Server { .. }) {
            self.disconnect();
        }
        Ok(())
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        set_timeout(&self.inner.write_timeout, timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        set_timeout(&self.inner.read_timeout, timeout)
    }

    /// The guard's end checks the client against what it carries and does not read `peer`; the
    /// bridge's checks the owner it carries and, for [`Peer::Process`], the server's identity.
    fn peer_is(&self, peer: Peer) -> bool {
        match &self.inner.end {
            End::Server { client, package } => {
                sid::client_is(&self.inner.handle, client, package.as_ref())
            }
            End::Client { server } => {
                sid::owner_of(&self.inner.handle).is_some_and(|owner| owner.same_as(server))
                    && match peer {
                        Peer::Process(guard) => identity::server_is(&self.inner.handle, guard),
                        Peer::SameUser => true,
                    }
            }
        }
    }
}

/// Sets a timeout shared by every clone, as a socket option is; zero is refused, as std's is.
fn set_timeout(slot: &Mutex<Option<Duration>>, timeout: Option<Duration>) -> io::Result<()> {
    if timeout.is_some_and(|timeout| timeout.is_zero()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a zero timeout",
        ));
    }
    *slot.lock().unwrap_or_else(PoisonError::into_inner) = timeout;
    Ok(())
}

/// A new manual-reset event, not signalled.
fn event() -> io::Result<OwnedHandle> {
    // SAFETY: no security attributes and no name; the handle returned is owned below.
    let raw = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }?;
    // SAFETY: `raw` is the event handle just created, which nothing else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw.0) })
}

/// A Windows error as std's: a Win32 code where the HRESULT carries one, so `raw_os_error` is
/// that code (the crate's own conversion keeps the whole HRESULT).
pub(super) fn io_error(error: &windows::core::Error) -> io::Error {
    let hresult = u32::from_ne_bytes(error.code().0.to_ne_bytes());
    if hresult & 0xFFFF_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error(i32::try_from(hresult & 0xFFFF).unwrap_or(i32::MAX))
    } else {
        io::Error::other(error.clone())
    }
}

/// A Win32 error as `raw_os_error` gives it.
pub(super) fn code(error: windows::Win32::Foundation::WIN32_ERROR) -> i32 {
    i32::try_from(error.0).unwrap_or(i32::MAX)
}
