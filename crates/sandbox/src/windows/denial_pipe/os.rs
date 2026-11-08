//! The denial pipe on Windows: the outer runner's client end ([`open_writer`]). The host's
//! server end is the launcher's (PR6, `launch/feed`).
#![expect(unsafe_code, reason = "FFI to the named pipe calls")]

use std::fs::File;
use std::io;
use std::os::windows::io::AsRawHandle as _;

use windows::Win32::Foundation::{GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, FILE_TYPE_PIPE, GetFileType,
    OPEN_EXISTING, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows::core::HSTRING;

use super::check_pipe_name;
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::pipe::os::into_file;

/// The client's access: `FILE_GENERIC_WRITE` less `FILE_CREATE_PIPE_INSTANCE`, and
/// `FILE_READ_ATTRIBUTES`. Nothing to read: the outer only writes.
const WRITE_END: u32 = (0x0012_0116 & !0x4) | 0x0080;

/// Opens the client end of the denial pipe `name` for the outer runner: the name checked first,
/// write-only, synchronous, not inheritable (no process the outer starts gets it), with
/// identification-only quality of service (the host may learn who connected but never act as
/// the lawyer through it), and checked to be a pipe. It takes the pipe's one instance.
pub fn open_writer(name: &str) -> io::Result<File> {
    check_pipe_name(name).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: false.into(),
    };
    // SAFETY: the name is a live HSTRING and `attributes` outlives the call; the handle
    // returned is owned by `Handle`.
    let handle = unsafe {
        CreateFileW(
            &HSTRING::from(name),
            WRITE_END,
            FILE_SHARE_NONE,
            Some(&raw const attributes),
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

/// Whether `file`'s handle would be inherited by a process started with inheritance.
pub fn inheritable(file: &File) -> io::Result<bool> {
    let mut flags = 0u32;
    // SAFETY: the handle is the open file's; `flags` is a live out-pointer.
    unsafe { GetHandleInformation(HANDLE(file.as_raw_handle()), &raw mut flags) }
        .map_err(|error| os_error(&error))?;
    Ok(flags & HANDLE_FLAG_INHERIT.0 != 0)
}
