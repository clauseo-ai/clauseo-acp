//! The denial pipe on Windows: the host's server end ([`DenialServer`]) and the outer runner's
//! client end ([`open_writer`], [`open_sink`]).
#![expect(unsafe_code, reason = "FFI to the named pipe calls")]

use std::fs::File;
use std::io;
use std::os::windows::io::RawHandle;
use std::time::Duration;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, FILE_TYPE_PIPE, GetFileType,
    OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows::core::HSTRING;

use super::sink::RecordSink;
use super::{check_pipe_name, denial_pipe_sddl};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::pipe::Role;
use crate::windows::pipe::os::{ServerPipe, into_file};
use crate::windows::sids::Sid;

/// The client's access: `FILE_GENERIC_WRITE` less `FILE_CREATE_PIPE_INSTANCE`, and
/// `FILE_READ_ATTRIBUTES`. Nothing to read: the outer only writes.
const WRITE_END: u32 = (0x0012_0116 & !0x4) | 0x0080;

/// The host's end of a denial pipe: made overlapped, so its wait for the runner can also watch
/// the runner's process.
pub struct DenialServer(ServerPipe);

impl DenialServer {
    /// Makes the pipe `name` (checked by [`check_pipe_name`]) for `lawyer`, the desktop user:
    /// [`denial_pipe_sddl`], one instance made first or refused, inbound, local clients only.
    pub fn create(name: &str, lawyer: &Sid) -> io::Result<DenialServer> {
        check_pipe_name(name)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        // `Err` is the runner pipes' inbound role: the server reads, the client writes.
        let pipe = ServerPipe::create(name, Role::Err, &denial_pipe_sddl(lawyer))?;
        Ok(DenialServer(pipe))
    }

    /// Waits up to `timeout` for the runner to connect, giving up early when `runner` (its
    /// process handle) is signalled.
    pub fn connect(&self, runner: RawHandle, timeout: Duration) -> io::Result<()> {
        self.0.connect(HANDLE(runner), timeout)
    }

    /// The connected client's pid: the host reads records only when it is the runner it started.
    pub fn client_pid(&self) -> io::Result<u32> {
        self.0.client_pid()
    }
}

impl io::Read for &DenialServer {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::Read::read(&mut &self.0, buf)
    }
}

/// Opens the client end of the denial pipe `name` for the outer runner: the name checked first,
/// write-only, synchronous, with identification-only quality of service (the host may learn who
/// connected but never act as the lawyer through it), and checked to be a pipe.
pub fn open_writer(name: &str) -> io::Result<File> {
    check_pipe_name(name).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    // SAFETY: the name is a live HSTRING; no security attributes; the handle returned is owned
    // by `Handle`.
    let handle = unsafe {
        CreateFileW(
            &HSTRING::from(name),
            WRITE_END,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0),
            None,
        )
    }
    .map_err(|error| os_error(&error))?;
    let handle = Handle(handle);
    // SAFETY: the handle is open.
    if unsafe { GetFileType(handle.0) } != FILE_TYPE_PIPE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the denial pipe's name opened something that isn't a pipe",
        ));
    }
    Ok(into_file(handle))
}

/// The outer's record sink: the launch's denial pipe, or, with none or one that can't be opened,
/// a sink that only logs.
pub(crate) fn open_sink(name: Option<&str>) -> RecordSink {
    let Some(name) = name else {
        tracing::warn!("the launch names no denial pipe: the git watch's records are only logged");
        return RecordSink::start(None);
    };
    match open_writer(name) {
        Ok(file) => RecordSink::start(Some(Box::new(file))),
        Err(error) => {
            tracing::warn!(%error, "the denial pipe can't be opened: records are only logged");
            RecordSink::start(None)
        }
    }
}
