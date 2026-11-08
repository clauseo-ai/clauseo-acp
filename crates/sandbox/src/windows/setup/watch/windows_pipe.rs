//! The watcher's pipe server and the key that publishes its name (`protocol.rs` says what both
//! are). Every instance is overlapped, so the serving loop never blocks on a client: a connect
//! is polled, a client's departure is seen by `PeekNamedPipe`, and a write that doesn't finish
//! within [`WRITE_WAIT_MS`] is cancelled and its client dropped. A client that connects and
//! leaves before it is serviced makes `ConnectNamedPipe` (or its completion) answer
//! `ERROR_NO_DATA` (232) or `ERROR_PIPE_NOT_CONNECTED` (233): the instance is disconnected and
//! listens again ([`Connect::Left`]), never a failure of the run; on a peek or a write, the
//! client is dropped. Which instance is made or closed when is `instances.rs`'s, which never
//! lets the count reach zero.
#![expect(
    unsafe_code,
    reason = "FFI to the named pipe, overlapped I/O, security and registry calls"
)]

use std::io;

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_NO_DATA,
    ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree,
    WAIT_OBJECT_0, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    GetSecurityInfo, SE_KERNEL_OBJECT, SE_OBJECT_TYPE, SE_REGISTRY_KEY,
};
use windows::Win32::Security::{
    IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    WinLocalSystemSid,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES,
    PIPE_ACCESS_DUPLEX, READ_CONTROL, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, PeekNamedPipe,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_CREATE_KEY_DISPOSITION,
    REG_CREATED_NEW_KEY, REG_OPTION_VOLATILE, REG_SAM_FLAGS, REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteKeyExW, RegOpenKeyExW, RegSetValueExW,
};
use windows::Win32::System::Threading::{CreateEventW, ResetEvent, WaitForSingleObject};
use windows::core::HSTRING;

use super::Failed;
use super::instances::{Connect, Instances, MAX_INSTANCES, Pipes};
use super::protocol::{KEY_SDDL, KeyStart, PIPE_VALUE, WATCH_KEY, key_start, pipe_name, pipe_sddl};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::sids::Sid;
use crate::windows::token::os::LocalDescriptor;

/// The out buffer: about 300 lines.
const OUT_BUFFER: u32 = 64 * 1024;
/// The in buffer: the client never writes (its DACL entry is `FR`).
const IN_BUFFER: u32 = 512;
/// How long one write may take before its client is dropped, in milliseconds.
const WRITE_WAIT_MS: u32 = 1000;

/// One instance. Dropped with its connect pending, it cancels the connect and waits for it, so
/// the kernel never writes into a freed `OVERLAPPED`.
pub(super) struct Instance {
    pipe: Handle,
    io: Box<OVERLAPPED>,
    _event: Handle,
    /// The connect is still outstanding.
    pending: bool,
    /// A client connected and left before it was serviced: the instance listens again.
    left: bool,
}

/// The real [`Pipes`]: instances of one name, with one descriptor.
pub(super) struct WinPipes {
    name: String,
    sddl: String,
}

/// The pipe's instances and the published name.
pub(super) struct Server {
    instances: Instances<WinPipes>,
    published: bool,
}

impl Server {
    /// Makes the first instance under a random name, which only `owner` may open, then
    /// publishes the name.
    pub(super) fn open(owner: &Sid) -> io::Result<Server> {
        let mut bits = [0u8; 16];
        getrandom::fill(&mut bits).map_err(|error| io::Error::other(error.to_string()))?;
        let pipes = WinPipes {
            name: pipe_name(&bits),
            sddl: pipe_sddl(owner),
        };
        let first = pipes.listen(true)?;
        publish(&pipes.name)?;
        Ok(Server {
            instances: Instances::new(pipes, first),
            published: true,
        })
    }

    /// Takes a client that connected, listens again, drops clients that left, and gives how
    /// many are connected. `Err` ends the run, with the name deleted.
    pub(super) fn poll(&mut self) -> Result<usize, Failed> {
        self.instances.poll()
    }

    /// Writes `line` to every client; one whose write fails or stalls is dropped.
    pub(super) fn send(&mut self, line: &[u8]) -> Result<(), Failed> {
        self.instances.send(line)
    }

    /// Deletes the published name, then closes every instance.
    pub(super) fn close(mut self) {
        if self.published {
            if let Err(error) = unpublish() {
                tracing::warn!(%error, "the watcher's pipe name wasn't deleted");
            }
            self.published = false;
        }
        self.instances.close_all();
    }
}

impl WinPipes {
    /// A new instance with a connect pending; the first is made first or refused.
    fn listen(&self, first: bool) -> io::Result<Instance> {
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
        let most = u32::try_from(MAX_INSTANCES).map_err(io::Error::other)?;
        // SAFETY: the name is a live HSTRING; `attributes` and its descriptor outlive the call;
        // the handle returned is owned by `Handle`.
        let pipe = unsafe {
            CreateNamedPipeW(
                &HSTRING::from(self.name.as_str()),
                // DUPLEX, not OUTBOUND: `PeekNamedPipe` (`alive`) needs read access on the server
                // end. Clients can't write all the same: the DACL gives them `FR` only.
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | only,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                most,
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
        let io = Box::new(OVERLAPPED {
            hEvent: event.0,
            ..OVERLAPPED::default()
        });
        let mut instance = Instance {
            pipe,
            io,
            _event: event,
            pending: false,
            left: false,
        };
        instance.connect()?;
        Ok(instance)
    }
}

impl Instance {
    /// Starts the connect. A client that connected and already left (232, 233) only marks the
    /// instance [`Instance::left`]; any other error is the caller's.
    fn connect(&mut self) -> io::Result<()> {
        // SAFETY: the instance's own manual-reset event, live with it.
        unsafe { ResetEvent(self.io.hEvent) }.map_err(|e| os_error(&e))?;
        *self.io = OVERLAPPED {
            hEvent: self.io.hEvent,
            ..OVERLAPPED::default()
        };
        // SAFETY: a live server end with no I/O outstanding; `io` is boxed and kept with the
        // pipe until the connect is done or cancelled and waited for (`Instance`'s drop).
        let started = unsafe { ConnectNamedPipe(self.pipe.0, Some(&raw mut *self.io)) };
        (self.pending, self.left) = match started {
            Ok(()) => (false, false),
            Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => (false, false),
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => (true, false),
            Err(error) if gone(error.code()) => (false, true),
            Err(error) => return Err(os_error(&error)),
        };
        Ok(())
    }
}

/// Whether `code` says the client connected and left before it was serviced.
fn gone(code: windows::core::HRESULT) -> bool {
    code == ERROR_NO_DATA.to_hresult() || code == ERROR_PIPE_NOT_CONNECTED.to_hresult()
}

impl Pipes for WinPipes {
    type Pipe = Instance;

    fn create(&mut self) -> Result<Instance, Failed> {
        self.listen(false)
            .map_err(|error| format!("the pipe couldn't listen again: {error}"))
    }

    fn owned_by_system(&mut self, pipe: &Instance) -> Result<bool, Failed> {
        // The server end's access holds READ_CONTROL (STANDARD_RIGHTS_READ and _WRITE).
        owner_is_system(pipe.pipe.0, SE_KERNEL_OBJECT)
            .map_err(|error| format!("the pipe's owner couldn't be read: {error}"))
    }

    fn accepted(&mut self, pipe: &mut Instance) -> Connect {
        if pipe.left {
            return Connect::Left;
        }
        if !pipe.pending {
            return Connect::Connected;
        }
        // SAFETY: the event is the connect's, live with the instance.
        if unsafe { WaitForSingleObject(pipe.io.hEvent, 0) } != WAIT_OBJECT_0 {
            return Connect::Waiting;
        }
        let mut moved = 0u32;
        // SAFETY: the connect was started with this OVERLAPPED on this pipe and has completed.
        let done =
            unsafe { GetOverlappedResult(pipe.pipe.0, &raw const *pipe.io, &raw mut moved, false) };
        pipe.pending = false;
        match done {
            Ok(()) => Connect::Connected,
            Err(error) if gone(error.code()) => {
                pipe.left = true;
                Connect::Left
            }
            Err(_) => Connect::Failed,
        }
    }

    fn relisten(&mut self, pipe: &mut Instance) -> Result<(), Failed> {
        // Only a finished connect is redone: `pending` is false whenever `left` is set.
        if pipe.pending {
            return Err("the pipe's connect is still outstanding".to_owned());
        }
        disconnect(&pipe.pipe);
        pipe.left = false;
        pipe.connect()
            .map_err(|error| format!("the pipe couldn't listen again in place: {error}"))
    }

    fn alive(&mut self, pipe: &Instance) -> Result<bool, Failed> {
        // SAFETY: a live server end; no buffer, so nothing is read.
        match unsafe { PeekNamedPipe(pipe.pipe.0, None, 0, None, None, None) } {
            Ok(()) => Ok(true),
            // The handle can't ask: every client would look gone. Fail loudly instead (74).
            Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => Err(format!(
                "PeekNamedPipe was refused on the server end: {error}"
            )),
            Err(_gone) => Ok(false),
        }
    }

    fn write(&mut self, pipe: &Instance, line: &[u8]) -> bool {
        write_all(&pipe.pipe, line)
    }

    fn close(&mut self, pipe: Instance) {
        if !pipe.pending {
            disconnect(&pipe.pipe);
        }
        drop(pipe);
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        if self.pending {
            let mut moved = 0u32;
            // SAFETY: the connect was started with this OVERLAPPED on this pipe; cancelling a
            // finished one does nothing, and the wait returns once it is done.
            unsafe {
                drop(CancelIoEx(self.pipe.0, Some(&raw const *self.io)));
                drop(GetOverlappedResult(
                    self.pipe.0,
                    &raw const *self.io,
                    &raw mut moved,
                    true,
                ));
            }
        }
    }
}

/// Whether the owner of `object` (open with `READ_CONTROL`) is LOCAL SYSTEM.
fn owner_is_system(object: HANDLE, kind: SE_OBJECT_TYPE) -> io::Result<bool> {
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the handle is open; both out-pointers are live; `owner` points into the
    // descriptor, which is freed below after the owner is compared.
    let code = unsafe {
        GetSecurityInfo(
            object,
            kind,
            OWNER_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            None,
            None,
            Some(&raw mut descriptor),
        )
    };
    checked(code)?;
    // SAFETY: `owner` is a valid SID inside the live descriptor, or null, which isn't SYSTEM.
    let system =
        !owner.0.is_null() && unsafe { IsWellKnownSid(owner, WinLocalSystemSid) }.as_bool();
    if !descriptor.0.is_null() {
        // SAFETY: allocated by GetSecurityInfo with LocalAlloc, freed once.
        unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    }
    Ok(system)
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

/// Publishes `name` as [`PIPE_VALUE`] in a fresh, volatile [`WATCH_KEY`] with [`KEY_SDDL`].
/// A key there already is a killed run's when SYSTEM owns it (one task instance runs at a time),
/// and is deleted; one anyone else owns is refused ([`key_start`]). A key that was there after
/// all (made in between, which only an administrator could) is refused too.
fn publish(name: &str) -> io::Result<()> {
    match key_start(key_owner_is_system()?) {
        KeyStart::Fresh => {}
        KeyStart::Stale => unpublish()?,
        KeyStart::Foreign => {
            return Err(io::Error::other(
                "the watch key is owned by someone other than SYSTEM",
            ));
        }
    }
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

/// Whether [`WATCH_KEY`]'s owner is SYSTEM: `None` when there is no key.
fn key_owner_is_system() -> io::Result<Option<bool>> {
    let mut key = HKEY::default();
    // SAFETY: the key's name is a live HSTRING and `key` a live out-pointer.
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(WATCH_KEY),
            None,
            REG_SAM_FLAGS(READ_CONTROL.0) | KEY_WOW64_64KEY,
            &raw mut key,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    checked(code)?;
    let system = owner_is_system(HANDLE(key.0), SE_REGISTRY_KEY);
    // SAFETY: a key this call opened.
    let _closed = unsafe { RegCloseKey(key) };
    system.map(Some)
}

/// Deletes [`WATCH_KEY`]; one that isn't there is fine. `--remove` calls it too, through the
/// task's removal (`refresh::task`), so a killed watcher's name doesn't outlive the task.
pub(crate) fn unpublish() -> io::Result<()> {
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
