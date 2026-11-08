//! The set-up pipe's server end on Windows ([`Pipe`]): `CreateNamedPipeW` in message mode, its
//! first instance made with `FILE_FLAG_FIRST_PIPE_INSTANCE` and every instance local only
//! (`PIPE_REJECT_REMOTE_CLIENTS`), overlapped so every wait has a limit; the client's token read at
//! identification level (`ImpersonateNamedPipeClient`, `OpenThreadToken`) and its session from
//! the pipe (`GetNamedPipeClientSessionId`).
//!
//! While a client is served, one spare instance waits for the next: it connects, sends its
//! request and waits for its reply while the set-up runs, and the name is never free for
//! another process to make while this one serves. A third client at once finds every instance
//! busy and waits (`WaitNamedPipeW`) until one is free.
#![expect(
    unsafe_code,
    reason = "FFI to the named pipe, overlapped I/O and token calls"
)]

use std::io;
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_IO_PENDING, ERROR_MORE_DATA, ERROR_PIPE_BUSY,
    ERROR_PIPE_CONNECTED, HANDLE,
};
use windows::Win32::Security::{RevertToSelf, SECURITY_ATTRIBUTES, TOKEN_QUERY};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientSessionId,
    ImpersonateNamedPipeClient, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_MESSAGE, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};
use windows::core::HSTRING;

use super::{ClientFacts, Pipe, READ_WAIT, REQUEST_CAP, Unmade};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::pipe::os::Overlapped;
use crate::windows::token::os::{LocalDescriptor, groups, integrity, restricted, user};

/// How many instances the pipe may have at once: the one served and the spare, with room.
const INSTANCES: u32 = 4;
/// The pipe's buffer size each way.
const BUFFER: u32 = 64 * 1024;
/// How long a reply's write, and the wait for the client to close after it, may each take.
const REPLY_WAIT: Duration = Duration::from_secs(5);

/// The pipe `name`'s server: the instance waiting for a client.
pub(crate) struct WindowsPipe {
    name: String,
    sddl: String,
    spare: Option<Handle>,
}

impl WindowsPipe {
    /// Makes `name` with the descriptor `sddl`, its first instance or nothing: a name another
    /// process made first is [`Unmade::Taken`].
    pub(crate) fn create(name: &str, sddl: &str) -> Result<WindowsPipe, Unmade> {
        let first = instance(name, sddl, true).map_err(|error| {
            let taken = [ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_PIPE_BUSY]
                .iter()
                .any(|code| error.raw_os_error() == i32::try_from(code.0).ok());
            if taken {
                Unmade::Taken
            } else {
                Unmade::Failed(error.to_string())
            }
        })?;
        Ok(WindowsPipe {
            name: name.to_owned(),
            sddl: sddl.to_owned(),
            spare: Some(first),
        })
    }
}

/// One instance of `name` with the descriptor `sddl`, overlapped, message mode, local clients
/// only; the first when `first`.
fn instance(name: &str, sddl: &str, first: bool) -> io::Result<Handle> {
    let descriptor = LocalDescriptor::new(sddl)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: descriptor.raw(),
        bInheritHandle: false.into(),
    };
    let mut open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: the name is a live HSTRING; `attributes` and the descriptor it points at outlive
    // the call; the handle returned is owned by `Handle`.
    let handle = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(name),
            open,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            INSTANCES,
            BUFFER,
            BUFFER,
            0,
            Some(&raw const attributes),
        )
    };
    if handle.is_invalid() {
        return Err(io::Error::last_os_error());
    }
    Ok(Handle(handle))
}

/// A connected client: its instance.
pub(crate) struct Client(Handle);

impl Pipe for WindowsPipe {
    type Client = Client;

    fn next(&mut self, wait: Duration) -> Result<Option<Client>, String> {
        // With no spare (its creation failed), no instance of this process's is left: a new one
        // is the first again, or nothing, so a name someone took meanwhile is never shared.
        let waiting = match self.spare.take() {
            Some(waiting) => waiting,
            None => instance(&self.name, &self.sddl, true).map_err(|e| e.to_string())?,
        };
        if !connect(&waiting, wait).map_err(|error| error.to_string())? {
            return Ok(None);
        }
        // The spare, made before this client is served, so the name stays this process's.
        match instance(&self.name, &self.sddl, false) {
            Ok(spare) => self.spare = Some(spare),
            Err(error) => tracing::warn!(%error, "no spare instance of the set-up pipe"),
        }
        Ok(Some(Client(waiting)))
    }

    fn request(&mut self, client: &mut Client) -> Result<Vec<u8>, String> {
        let mut buffer = vec![0u8; REQUEST_CAP + 1];
        let mut io = Overlapped::new().map_err(|error| error.to_string())?;
        // SAFETY: `buffer` and `io` outlive the operation, which `within` completes or cancels
        // before either drops.
        let started =
            unsafe { ReadFile(client.0.0, Some(&mut buffer), None, Some(io.as_mut_ptr())) };
        match started {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {}
            Err(error) if error.code() == ERROR_MORE_DATA.to_hresult() => {
                return Err(format!("a request of more than {REQUEST_CAP} bytes"));
            }
            Err(error) => return Err(os_error(&error).to_string()),
        }
        match io.within(&client.0, READ_WAIT) {
            Ok(Some(read)) if read <= REQUEST_CAP => {
                buffer.truncate(read);
                Ok(buffer)
            }
            Ok(Some(_)) => Err(format!("a request of more than {REQUEST_CAP} bytes")),
            Ok(None) => Err("no request in time".to_owned()),
            Err(error) if error.raw_os_error() == i32::try_from(ERROR_MORE_DATA.0).ok() => {
                Err(format!("a request of more than {REQUEST_CAP} bytes"))
            }
            Err(error) => Err(error.to_string()),
        }
    }

    fn facts(&mut self, client: &mut Client) -> Result<ClientFacts, String> {
        let token = client_token(&client.0).map_err(|error| error.to_string())?;
        let mut session = 0u32;
        // SAFETY: the pipe is a live server end with a client; `session` is a live out-pointer.
        unsafe { GetNamedPipeClientSessionId(client.0.0, &raw mut session) }
            .map_err(|error| os_error(&error).to_string())?;
        let facts = (|| {
            Ok::<_, io::Error>(ClientFacts {
                user: user(&token)?,
                session,
                groups: groups(&token)?.into_iter().map(|(sid, _)| sid).collect(),
                restricted: restricted(&token),
                integrity: integrity(&token)?,
            })
        })();
        facts.map_err(|error| error.to_string())
    }

    fn reply(&mut self, client: Client, line: &[u8]) {
        if let Err(error) = send(&client.0, line) {
            tracing::warn!(%error, "the set-up pipe's reply wasn't sent");
        }
        // SAFETY: a live server end; whatever the client hadn't read by now is dropped.
        if let Err(error) = unsafe { DisconnectNamedPipe(client.0.0) } {
            tracing::warn!(error = %os_error(&error), "the set-up pipe's client wasn't let go");
        }
    }
}

/// Waits up to `wait` for a client on `pipe`: `false` when none came.
fn connect(pipe: &Handle, wait: Duration) -> io::Result<bool> {
    let mut io = Overlapped::new()?;
    // SAFETY: the pipe is a live server end; `io` outlives the operation, which `within`
    // completes or cancels before `io` drops.
    match unsafe { ConnectNamedPipe(pipe.0, Some(io.as_mut_ptr())) } {
        Ok(()) => return Ok(true),
        Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => return Ok(true),
        Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {}
        Err(error) => return Err(os_error(&error)),
    }
    Ok(io.within(pipe, wait)?.is_some())
}

/// The client's token, at the level it allowed (identification is enough to read it): this
/// thread impersonates the client only for `OpenThreadToken`, opened as this process
/// (`OpenAsSelf`, which an identification token needs), then is itself again.
fn client_token(pipe: &Handle) -> io::Result<Handle> {
    // SAFETY: a live server end that has read the client's message, as the call requires.
    unsafe { ImpersonateNamedPipeClient(pipe.0) }.map_err(|error| os_error(&error))?;
    let mut token = HANDLE::default();
    // SAFETY: GetCurrentThread is a pseudo-handle; `token` is a live out-pointer, owned by a
    // `Handle` below.
    let opened = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &raw mut token) };
    // SAFETY: no arguments; it ends this thread's impersonation of the client.
    if unsafe { RevertToSelf() }.is_err() {
        // A thread that can't stop acting as the client can't be trusted with the set-up.
        std::process::abort();
    }
    opened.map_err(|error| os_error(&error))?;
    Ok(Handle(token))
}

/// Writes `line` to the client within [`REPLY_WAIT`], then waits as long again for the client
/// to close its end, so the reply is read before the instance lets it go.
fn send(pipe: &Handle, line: &[u8]) -> io::Result<()> {
    let mut io = Overlapped::new()?;
    // SAFETY: `line` and `io` outlive the operation, which `within` completes or cancels.
    let started = unsafe { WriteFile(pipe.0, Some(line), None, Some(io.as_mut_ptr())) };
    match started {
        Ok(()) => {}
        Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {}
        Err(error) => return Err(os_error(&error)),
    }
    if io.within(pipe, REPLY_WAIT)?.is_none() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the reply wasn't taken",
        ));
    }
    let mut rest = [0u8; 16];
    let mut io = Overlapped::new()?;
    // SAFETY: `rest` and `io` outlive the operation, which `within` completes or cancels. It
    // ends when the client closes its end (an error, which is the point) or sends more.
    let started = unsafe { ReadFile(pipe.0, Some(&mut rest), None, Some(io.as_mut_ptr())) };
    if started.is_err_and(|error| error.code() == ERROR_IO_PENDING.to_hresult()) {
        drop(io.within(pipe, REPLY_WAIT));
    }
    Ok(())
}
