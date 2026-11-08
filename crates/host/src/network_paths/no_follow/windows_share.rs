//! A share matter's root, opened on Windows as the lawyer stored it ([`super::MatterRoot`]):
//! `CreateFileW` on that one text, links on it followed as the system does (the lawyer
//! configured it; an agent names nothing here), checked to be a folder. Every name below it is
//! opened from this handle with no link followed (`super::sys`), so the root is the only text
//! the system resolves.
#![expect(unsafe_code, reason = "FFI to CreateFileW for a share matter's root")]

use std::io;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{FromRawHandle as _, OwnedHandle};
use std::path::Path;

use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING, SYNCHRONIZE,
};
use windows::core::PCWSTR;

use super::Kind;
use super::sys::{Handle, win_error};

/// Opens the share matter's root `path` (its stored text, admitted by
/// [`super::matter_root::share_matter_root`]) to list, a folder or an error.
pub(super) fn open(path: &Path) -> io::Result<Handle> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(once(0)).collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes or template
    // are passed; OPEN_EXISTING never creates anything. Without FILE_FLAG_OVERLAPPED the handle is
    // synchronous, as the listing's `NtQueryDirectoryFile` needs.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            (FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(|error| win_error(&error))?;
    // SAFETY: the call succeeded, so `handle` is a new open handle this function now owns.
    let opened = Handle::from_owned(unsafe { OwnedHandle::from_raw_handle(handle.0) });
    if opened.kind()? != Kind::Dir {
        return Err(io::Error::from(io::ErrorKind::NotADirectory));
    }
    Ok(opened)
}
