//! File identity for a matter's Codex login link (`state_folder.rs`): whether two names are one
//! file, read from the file system's own id and never by opening either file for its contents.
//! The same rule as `clauseo_agents::same_file` (S18), which the host may not depend on
//! (ARCHITECTURE.md): links are not followed, so a symbolic link or junction in either place is
//! never "the same file".

use std::io;
use std::path::Path;

/// Whether `a` and `b` are one file: the same device and inode on Unix, the same volume serial
/// number and file index on Windows. False when either is a symbolic link, a junction or any
/// other reparse point. Err when either cannot be read (a missing file included, whose error
/// is `NotFound` on Windows too: [`os_error`]).
///
/// Unix reads `symlink_metadata` (`lstat`), which opens nothing. Windows opens a handle to each
/// path with `CreateFileW` and desired access 0 (no right to the contents), sharing read, write
/// and delete so no other program is blocked, with `FILE_FLAG_OPEN_REPARSE_POINT` so a link is
/// opened as itself; the handle is closed before this returns.
pub(crate) fn same_file(a: &Path, b: &Path) -> io::Result<bool> {
    let (Some((a, _)), Some((b, _))) = (file_id(a)?, file_id(b)?) else {
        return Ok(false);
    };
    Ok(a == b)
}

/// How many names the file at `path` has (its hard links), read as [`same_file`] reads ids;
/// `None` for a symbolic link or other reparse point. Err when it cannot be read.
#[cfg(windows)]
pub(crate) fn link_count(path: &Path) -> io::Result<Option<u64>> {
    Ok(file_id(path)?.map(|(_, links)| links))
}

/// The file's id, (device, inode), and its link count. `None` for a symbolic link.
#[cfg(unix)]
fn file_id(path: &Path) -> io::Result<Option<((u64, u64), u64)>> {
    use std::os::unix::fs::MetadataExt as _;

    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(None);
    }
    Ok(Some(((meta.dev(), meta.ino()), meta.nlink())))
}

/// The file's id, (volume serial number, file index), and its link count. `None` for a reparse
/// point.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "CreateFileW and GetFileInformationByHandle are the OS's file-id calls; std has no stable one"
)]
fn file_id(path: &Path) -> io::Result<Option<((u64, u64), u64)>> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
    };
    use windows::core::PCWSTR;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call; the security
    // attributes and the template are null, which the function allows. Desired access 0 asks
    // for no right to the contents.
    let raw = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|error| os_error(&error))?;
    // SAFETY: `raw` is a valid handle CreateFileW just returned, owned by nothing else; the
    // `OwnedHandle` closes it once, when dropped.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is open for the call's duration, and `info` is a valid, writable
    // BY_HANDLE_FILE_INFORMATION.
    unsafe { GetFileInformationByHandle(HANDLE(handle.as_raw_handle()), &raw mut info) }
        .map_err(|error| os_error(&error))?;
    drop(handle);
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Ok(None);
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    let id = (u64::from(info.dwVolumeSerialNumber), index);
    Ok(Some((id, u64::from(info.nNumberOfLinks))))
}

/// The Win32 error inside `error` as std's own OS error, so `kind()` is right: a missing file
/// (`ERROR_FILE_NOT_FOUND`, `ERROR_PATH_NOT_FOUND`) reads as `NotFound`, which
/// `safe_read::read_plain` takes for an absent file. `io::Error::from` would keep the HRESULT
/// (`0x80070002`), whose kind std does not know, and so failed every Claude launch whose state
/// folder had no `.claude.json` or `settings.json` yet. The rule of `clauseo_mcp`'s Windows
/// write, narrowed by [`win32_error`]. An error that is no Win32 error keeps its HRESULT, whose
/// kind is never `NotFound`.
#[cfg(windows)]
pub(super) fn os_error(error: &windows::core::Error) -> io::Error {
    use windows::Win32::Foundation::WIN32_ERROR;
    WIN32_ERROR::from_error(error).map_or_else(|| io::Error::from(error.clone()), win32_error)
}

/// The Win32 error `code` as an io error that reads as `NotFound` only for
/// `ERROR_FILE_NOT_FOUND` (2) and `ERROR_PATH_NOT_FOUND` (3), so the read of a state file fails
/// closed: std reads `ERROR_INVALID_DRIVE` (15), `ERROR_BAD_NETPATH` (53) and
/// `ERROR_BAD_NET_NAME` (67) as `NotFound` too, and those become `Other`, the OS error kept as
/// the cause. Access denied, a sharing violation, a reparse refusal and every other code keep
/// std's kind.
#[cfg(windows)]
fn win32_error(code: windows::Win32::Foundation::WIN32_ERROR) -> io::Error {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    let Ok(raw) = i32::try_from(code.0) else {
        return io::Error::other(format!("Windows error {}", code.0));
    };
    let error = io::Error::from_raw_os_error(raw);
    let missing = code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND;
    if error.kind() == io::ErrorKind::NotFound && !missing {
        io::Error::other(error)
    } else {
        error
    }
}
