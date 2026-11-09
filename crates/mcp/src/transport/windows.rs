//! Windows only: the named-pipe transport, `\\.\pipe\<name>`. The guard serves it with
//! `browser-guard --listen-pipe <name> --client-sid <SID> [--client-package-sid <SID>]`
//! ([`PipeListener`]); the bridge reaches it with `browser-bridge --pipe <name> --server-sid <SID>
//! --guard-pid <pid> --guard-pidversion <version>` ([`connect_within`]). The guard runs as the
//! browser's account, the bridge as the agent's, whose token is Low integrity, and soon an
//! `AppContainer` (`LowBox`) token of the agent's own package.
//!
//! [`PipeListener::make`] creates the pipe's first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE`,
//! so a name someone else holds already fails ([`MakeError::Taken`]; the guard exits 3 at once and
//! the host retries with a new name). The agent can make a pipe of any free name, even at Low
//! integrity (the pipe root lets it add one), so it can squat a name it guesses: the host's random
//! name, the bridge's owner check (a squatted pipe is the agent's own, and refused) and that retry
//! are the defence, not the pipe root. Every instance is byte mode, rejects remote clients, and
//! gets the descriptor [`sddl`] writes: owned by the guard's user, whose DACL grants that user full
//! access and the client SID exactly read, write-data and read-control (never create-instance, so
//! the agent cannot add an instance of its own), the same to the client package SID when given (an
//! `AppContainer`'s access check needs both its user and its package to match), with no other entry
//! (never ALL APPLICATION PACKAGES, which would let Chrome's own `LowBox` processes in), and the
//! Low mandatory label with no-write-up, which a Low-integrity agent may open for writing. The
//! guard always keeps an instance waiting, so the name never lapses while it runs.
//!
//! The guard's peer check impersonates each client (`sid.rs`) and compares its token's user with
//! the client SID and, when a package SID is given, requires an `AppContainer` token of that
//! package with no capability. The bridge opens the pipe at identification level (the guard can
//! learn who it is and do nothing as it) and refuses one not owned by the server SID, such as a
//! pipe the agent's own account made first, or whose server process is not the guard's by pid and
//! creation time (`identity.rs`), such as an instance agent-browser added under the browser's
//! account. Before it listens the guard lets the client query its process for that
//! (`identity::open_to`). Neither side logs a pipe name or a SID.
//!
//! `PIPE_REJECT_REMOTE_CLIENTS` is set at creation (`PipeListener::instance`); no Windows call
//! reads it back (`GetNamedPipeInfo` reports only the type and the end), so the tests check the
//! other flags.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 named-pipe APIs, for the guard's pipe and the bridge's connect"
)]

#[cfg(test)]
mod access_tests;
mod identity;
mod pipe;
mod sid;
#[cfg(test)]
mod tests;

use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_PIPE_BUSY, GENERIC_READ};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ,
    FILE_SHARE_NONE, FILE_WRITE_DATA, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT,
};
use windows::Win32::System::Pipes::{
    CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    WaitNamedPipeW,
};
use windows::core::{BOOL, PCWSTR};

use super::{Listener, checks};
use crate::browser_guard::now;
pub(crate) use identity::own as own_identity;
pub(crate) use pipe::Pipe;
use pipe::{End, code, io_error};
pub(crate) use sid::Sid;

/// The most instances at once: the client, one being refused, and the one waiting. A refused
/// client holds its instance for up to about 2 s (the guard waits for its first request, then
/// writes the refusal), so a burst of refused clients can use them up for that long; only the
/// client account can connect, so that is the agent denying itself, never anyone else.
const MAX_INSTANCES: u32 = 4;
/// Each direction's buffer.
const BUFFER: u32 = 64 * 1024;
/// What the client SID may do: `FILE_GENERIC_READ` (read data, attributes and extended
/// attributes, read-control, synchronize) and write data. Not `GENERIC_WRITE`, which includes
/// `FILE_APPEND_DATA`, the pipe's create-instance right.
const CLIENT_RIGHTS: u32 = FILE_GENERIC_READ.0 | FILE_WRITE_DATA.0;

/// Why [`PipeListener::make`] failed.
#[derive(Debug)]
pub(crate) enum MakeError {
    /// A pipe of that name exists already (someone else's, or an earlier guard's).
    Taken,
    /// `--client-sid` is not a SID Windows takes, or `--client-package-sid` not an
    /// `AppContainer` one.
    BadSid,
    /// Anything else, with the reason, which names no pipe and no SID.
    Failed(String),
}

/// The guard's pipe: the name, the descriptor every instance gets, the client SID, and the
/// instance waiting for the next client.
pub(crate) struct PipeListener {
    path: Vec<u16>,
    sddl: Vec<u16>,
    client: Sid,
    package: Option<Sid>,
    waiting: Mutex<Option<Pipe>>,
}

impl PipeListener {
    /// Creates the pipe `\\.\pipe\<name>`, its first instance, for clients running as
    /// `client_sid`, in the `AppContainer` package `package_sid` when given, and lets them query
    /// this process ([`identity::open_to`]). Call it before anything is claimed.
    pub(crate) fn make(
        name: &str,
        client_sid: &str,
        package_sid: Option<&str>,
    ) -> Result<PipeListener, MakeError> {
        if !checks::pipe_name_ok(name) {
            return Err(MakeError::Failed(
                "the --listen-pipe name must be letters, digits and dashes".to_owned(),
            ));
        }
        let client = Sid::parse(client_sid).ok_or(MakeError::BadSid)?;
        let package = match package_sid {
            Some(text) if checks::package_sid_text_ok(text) => {
                Some(Sid::parse(text).ok_or(MakeError::BadSid)?)
            }
            Some(_) => return Err(MakeError::BadSid),
            None => None,
        };
        let own = sid::current_user()
            .map_err(|error| MakeError::Failed(format!("cannot read the guard's user: {error}")))?;
        let package_text = package.as_ref().map(Sid::text).transpose();
        let (Ok(client_text), Ok(own_text), Ok(package_text)) =
            (client.text(), own.text(), package_text)
        else {
            return Err(MakeError::Failed("cannot write the pipe's SIDs".to_owned()));
        };
        let listener = PipeListener {
            path: pipe_path(name),
            sddl: sid::wide(&sddl(&own_text, &client_text, package_text.as_deref())),
            client,
            package,
            waiting: Mutex::new(None),
        };
        let first = listener
            .instance(true)
            .map_err(|error| match error.raw_os_error() {
                Some(raw) if raw == code(ERROR_ACCESS_DENIED) || raw == code(ERROR_PIPE_BUSY) => {
                    MakeError::Taken
                }
                _ => MakeError::Failed(format!("cannot create the pipe: {error}")),
            })?;
        *listener.lock() = Some(first);
        let mut clients = vec![&listener.client];
        clients.extend(listener.package.as_ref());
        identity::open_to(&clients).map_err(|error| {
            MakeError::Failed(format!(
                "cannot let the client query the guard's process: {error}"
            ))
        })?;
        Ok(listener)
    }

    /// A new instance of the pipe, not yet connected; `first` refuses a name that exists.
    fn instance(&self, first: bool) -> io::Result<Pipe> {
        let descriptor = sid::Descriptor::from_sddl(&self.sddl)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
            lpSecurityDescriptor: descriptor.as_ptr(),
            bInheritHandle: false.into(),
        };
        let mut open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
        if first {
            open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        // SAFETY: `path` is NUL-terminated and `attributes` points to a descriptor that lives
        // until the call returns; the handle returned is owned below.
        let raw = unsafe {
            CreateNamedPipeW(
                PCWSTR(self.path.as_ptr()),
                open,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                MAX_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                Some(&raw const attributes),
            )
        };
        if raw.is_invalid() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `raw` is the pipe handle just created, which nothing else owns.
        let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
        Pipe::new(
            handle,
            End::Server {
                client: self.client.clone(),
                package: self.package.clone(),
            },
        )
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Pipe>> {
        // A panicked holder left an instance or none; either is fine.
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Listener for PipeListener {
    type Connection = Pipe;

    /// Waits for a client on the waiting instance, then makes the next one before handing this
    /// one over, so an instance is always waiting. When the next cannot be made, the client is
    /// dropped and its instance waits again.
    fn accept(&self) -> io::Result<Pipe> {
        let mut waiting = self.lock();
        let current = match waiting.take() {
            Some(current) => current,
            None => self.instance(false)?,
        };
        if let Err(error) = current.connect() {
            current.disconnect();
            *waiting = Some(current);
            return Err(error);
        }
        match self.instance(false) {
            Ok(next) => {
                *waiting = Some(next);
                Ok(current)
            }
            Err(error) => {
                current.disconnect();
                *waiting = Some(current);
                Err(error)
            }
        }
    }
}

/// The pipe's security descriptor: owner `own`, a protected DACL granting `own` everything and
/// `client` (and `package`, when given) [`CLIENT_RIGHTS`] and nothing to anyone else, and the
/// Low no-write-up label. Every SID is a canonical string Windows wrote (`Sid::text`), so nothing
/// else enters the string.
fn sddl(own: &str, client: &str, package: Option<&str>) -> String {
    let package = package.map_or_else(String::new, |package| {
        format!("(A;;{CLIENT_RIGHTS:#x};;;{package})")
    });
    format!("O:{own}D:P(A;;{CLIENT_RIGHTS:#x};;;{client}){package}(A;;GA;;;{own})S:(ML;;NW;;;LW)")
}

/// `\\.\pipe\<name>`, NUL-terminated.
fn pipe_path(name: &str) -> Vec<u16> {
    sid::wide(&format!(r"\\.\pipe\{name}"))
}

/// The bridge's connect: opens `\\.\pipe\<name>` at identification level, waiting while every
/// instance is busy, up to `within`. The bridge then runs its peer check on it
/// (`Connection::peer_is`: the owner must be `server`, the server process the guard) and closes
/// one that fails.
pub(crate) fn connect_within(name: &str, server: &Sid, within: Duration) -> io::Result<Pipe> {
    if !checks::pipe_name_ok(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a pipe name",
        ));
    }
    let path = pipe_path(name);
    let deadline = now() + within;
    let handle = loop {
        // SAFETY: `path` is NUL-terminated and outlives the call; the handle returned is owned
        // below.
        let opened = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                GENERIC_READ.0 | FILE_WRITE_DATA.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            )
        };
        match opened {
            // SAFETY: `raw` is the handle just opened, which nothing else owns.
            Ok(raw) => break unsafe { OwnedHandle::from_raw_handle(raw.0) },
            Err(error) if error.code() == ERROR_PIPE_BUSY.to_hresult() => {
                let left = deadline.saturating_duration_since(now());
                if left.is_zero() {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                let millis = u32::try_from(left.as_millis()).map_or(u32::MAX - 1, |ms| ms.max(1));
                // SAFETY: `path` is NUL-terminated and outlives the call. Its answer does not
                // matter: the next open tries again, or the deadline passes.
                let _waited: BOOL = unsafe { WaitNamedPipeW(PCWSTR(path.as_ptr()), millis) };
            }
            Err(error) => return Err(io_error(&error)),
        }
    };
    Pipe::new(
        handle,
        End::Client {
            server: server.clone(),
        },
    )
}
