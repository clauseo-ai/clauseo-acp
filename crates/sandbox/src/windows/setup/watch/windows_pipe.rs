//! The watcher's pipe server and the key that publishes its name (`protocol.rs` says what both
//! are). Every instance is overlapped, so the serving loop never blocks on a client: a connect
//! is polled, a client's departure is seen by `PeekNamedPipe`, and a write that doesn't finish
//! within [`WRITE_WAIT_MS`] is cancelled and its client dropped.
#![expect(
    unsafe_code,
    reason = "FFI to the named pipe, overlapped I/O and registry calls"
)]

use std::io;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, ERROR_SUCCESS, WAIT_OBJECT_0,
    WIN32_ERROR,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES,
    PIPE_ACCESS_DUPLEX, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, PeekNamedPipe,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_CREATE_KEY_DISPOSITION,
    REG_CREATED_NEW_KEY, REG_OPTION_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteKeyExW, RegSetValueExW,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::HSTRING;

use super::protocol::{KEY_SDDL, PIPE_VALUE, WATCH_KEY, pipe_name, pipe_sddl};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::sids::Sid;
use crate::windows::token::os::LocalDescriptor;

/// The most instances, so the most clients at once less one listening.
const MAX_INSTANCES: u32 = 8;
/// The out buffer: about 300 lines.
const OUT_BUFFER: u32 = 64 * 1024;
/// The in buffer: the client never writes.
const IN_BUFFER: u32 = 512;
/// How long one write may take before its client is dropped, in milliseconds.
const WRITE_WAIT_MS: u32 = 1000;

/// An instance waiting for its client. Dropped with its connect pending, it cancels the connect
/// and waits for it, so the kernel never writes into a freed `OVERLAPPED`.
struct Listener {
    /// `None` once handed to the clients.
    pipe: Option<Handle>,
    io: Box<OVERLAPPED>,
    _event: Handle,
    /// `ConnectNamedPipe` said the client was already there.
    connected: bool,
}

/// The pipe's instances and the published name.
pub(super) struct Server {
    name: String,
    sddl: String,
    listener: Option<Listener>,
    clients: Vec<Handle>,
    published: bool,
}

impl Server {
    /// Makes the first instance under a random name, which only `owner` may open, then
    /// publishes the name.
    pub(super) fn open(owner: &Sid) -> io::Result<Server> {
        let mut bits = [0u8; 16];
        getrandom::fill(&mut bits).map_err(|error| io::Error::other(error.to_string()))?;
        let mut server = Server {
            name: pipe_name(&bits),
            sddl: pipe_sddl(owner),
            listener: None,
            clients: Vec::new(),
            published: false,
        };
        server.listener = Some(server.listen(true)?);
        publish(&server.name)?;
        server.published = true;
        Ok(server)
    }

    /// A new instance with a connect pending; the first is made first or refused.
    fn listen(&self, first: bool) -> io::Result<Listener> {
        let descriptor = LocalDescriptor::new(&self.sddl)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
            lpSecurityDescriptor: descriptor.raw(),
            bInheritHandle: false.into(),
        };
        let only = if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            FILE_FLAGS_AND_ATTRIBUTES(0)
        };
        // SAFETY: the name is a live HSTRING; `attributes` and its descriptor outlive the call;
        // the handle returned is owned by `Handle`.
        let pipe = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(self.name.as_str()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | only,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                MAX_INSTANCES,
                OUT_BUFFER,
                IN_BUFFER,
                0,
                Some(&raw const attributes),
            )
        };
        if pipe.is_invalid() {
            return Err(io::Error::last_os_error());
        }
        let pipe = Handle(pipe);
        // SAFETY: no attributes and no name; the event is owned by `Handle`.
        let event =
            Handle(unsafe { CreateEventW(None, true, false, None) }.map_err(|e| os_error(&e))?);
        let mut io = Box::new(OVERLAPPED {
            hEvent: event.0,
            ..OVERLAPPED::default()
        });
        // SAFETY: a live server end; `io` is boxed and kept with the pipe until the connect is
        // done or the pipe is closed, which ends it.
        let connected = match unsafe { ConnectNamedPipe(pipe.0, Some(&raw mut *io)) } {
            Ok(()) => true,
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => true,
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => false,
            Err(error) => return Err(os_error(&error)),
        };
        Ok(Listener {
            pipe: Some(pipe),
            io,
            _event: event,
            connected,
        })
    }

    /// Takes a client that connected, listens again, drops clients that left, and gives how
    /// many are connected.
    pub(super) fn poll(&mut self) -> usize {
        if let Some(mut listener) = self.listener.take() {
            match accepted(&listener) {
                Some(true) => self.clients.extend(listener.pipe.take()),
                Some(false) => self.listener = Some(listener),
                // A connect that failed: the instance goes, another listens.
                None => {}
            }
        }
        self.clients.retain(|client| {
            // SAFETY: a live server end; no buffer, so nothing is read.
            let alive = unsafe { PeekNamedPipe(client.0, None, 0, None, None, None) }.is_ok();
            if !alive {
                disconnect(client);
            }
            alive
        });
        if self.listener.is_none() && self.clients.len() < (MAX_INSTANCES - 1) as usize {
            match self.listen(false) {
                Ok(listener) => self.listener = Some(listener),
                Err(error) => tracing::warn!(%error, "the watcher's pipe couldn't listen again"),
            }
        }
        self.clients.len()
    }

    /// Writes `line` to every client; one whose write fails or stalls is dropped.
    pub(super) fn send(&mut self, line: &[u8]) {
        self.clients.retain(|client| {
            let sent = write_all(client, line);
            if !sent {
                disconnect(client);
            }
            sent
        });
    }

    /// Deletes the published name; the instances close as the server drops.
    pub(super) fn close(mut self) {
        if self.published {
            if let Err(error) = unpublish() {
                tracing::warn!(%error, "the watcher's pipe name wasn't deleted");
            }
            self.published = false;
        }
        for client in &self.clients {
            disconnect(client);
        }
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if let Some(pipe) = &self.pipe {
            let mut moved = 0u32;
            // SAFETY: the connect, if still pending, was started with this OVERLAPPED on this
            // pipe; cancelling a finished one does nothing, and the wait returns once it is done.
            unsafe {
                drop(CancelIoEx(pipe.0, Some(&raw const *self.io)));
                drop(GetOverlappedResult(
                    pipe.0,
                    &raw const *self.io,
                    &raw mut moved,
                    true,
                ));
            }
        }
    }
}

/// Whether `listener`'s client has connected: `None` when the connect failed.
fn accepted(listener: &Listener) -> Option<bool> {
    if listener.connected {
        return Some(true);
    }
    let pipe = listener.pipe.as_ref()?;
    // SAFETY: the event is the connect's, live with the listener.
    if unsafe { WaitForSingleObject(listener.io.hEvent, 0) } != WAIT_OBJECT_0 {
        return Some(false);
    }
    let mut moved = 0u32;
    // SAFETY: the connect was started with this OVERLAPPED on this pipe and has completed.
    let done =
        unsafe { GetOverlappedResult(pipe.0, &raw const *listener.io, &raw mut moved, false) };
    done.is_ok().then_some(true)
}

/// Writes all of `line` to `client`, waiting up to [`WRITE_WAIT_MS`]: whether it went.
fn write_all(client: &Handle, line: &[u8]) -> bool {
    // SAFETY: no attributes and no name; the event is owned by `Handle`.
    let Ok(event) = (unsafe { CreateEventW(None, true, false, None) }) else {
        return false;
    };
    let event = Handle(event);
    let mut io = Box::new(OVERLAPPED {
        hEvent: event.0,
        ..OVERLAPPED::default()
    });
    // SAFETY: `line` and `io` outlive the write, which is waited for or cancelled and waited
    // for below before either drops.
    let started = unsafe { WriteFile(client.0, Some(line), None, Some(&raw mut *io)) };
    match started {
        Ok(()) => {}
        Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {}
        Err(_) => return false,
    }
    // SAFETY: the event is live.
    let finished = unsafe { WaitForSingleObject(event.0, WRITE_WAIT_MS) } == WAIT_OBJECT_0;
    if !finished {
        // SAFETY: the write was started with this OVERLAPPED on `client`.
        drop(unsafe { CancelIoEx(client.0, Some(&raw const *io)) });
    }
    let mut moved = 0u32;
    // SAFETY: as above; waiting for the write (or its cancel) to finish.
    let done = unsafe { GetOverlappedResult(client.0, &raw const *io, &raw mut moved, true) };
    finished && done.is_ok() && usize::try_from(moved).is_ok_and(|moved| moved == line.len())
}

/// Disconnects `client`; its handle closes when it drops.
fn disconnect(client: &Handle) {
    // SAFETY: a live server end.
    drop(unsafe { DisconnectNamedPipe(client.0) });
}

/// `code` as an `io::Error` unless it is success.
fn checked(code: WIN32_ERROR) -> io::Result<()> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            i32::try_from(code.0).unwrap_or(i32::MAX),
        ))
    }
}

/// Publishes `name` as [`PIPE_VALUE`] in a fresh, volatile [`WATCH_KEY`] with [`KEY_SDDL`]:
/// one left by an earlier run is deleted first, and a key that was there after all (made in
/// between, which only an administrator could) is refused.
fn publish(name: &str) -> io::Result<()> {
    unpublish()?;
    let descriptor = LocalDescriptor::new(KEY_SDDL)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: descriptor.raw(),
        bInheritHandle: false.into(),
    };
    let mut key = HKEY::default();
    let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
    // SAFETY: the key's name is a live HSTRING; `attributes` and its descriptor, `key` and
    // `disposition` outlive the call.
    checked(unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(WATCH_KEY),
            None,
            None,
            REG_OPTION_VOLATILE,
            KEY_SET_VALUE | KEY_WOW64_64KEY,
            Some(&raw const attributes),
            &raw mut key,
            Some(&raw mut disposition),
        )
    })?;
    let written = if disposition == REG_CREATED_NEW_KEY {
        let text: Vec<u8> = name
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        // SAFETY: a key just made with KEY_SET_VALUE; the value's name and data outlive it.
        checked(unsafe {
            RegSetValueExW(key, &HSTRING::from(PIPE_VALUE), None, REG_SZ, Some(&text))
        })
    } else {
        Err(io::Error::other("the watch key was made by someone else"))
    };
    // SAFETY: a key this call opened.
    let _closed = unsafe { RegCloseKey(key) };
    written
}

/// Deletes [`WATCH_KEY`]; one that isn't there is fine.
fn unpublish() -> io::Result<()> {
    // SAFETY: the key's name is a live HSTRING.
    let code = unsafe {
        RegDeleteKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(WATCH_KEY),
            KEY_WOW64_64KEY.0,
            None,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    checked(code)
}
