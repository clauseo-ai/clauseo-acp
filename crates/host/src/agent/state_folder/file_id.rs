//! File identity for a matter's Codex login link (`state_folder.rs`): whether two names are one
//! file, read from the file system's own id and never by opening either file for its contents.
//! The same rule as `clauseo_agents::same_file` (S18), which the host may not depend on
//! (ARCHITECTURE.md): links are not followed, so a symbolic link or junction in either place is
//! never "the same file".

use std::io;
use std::path::Path;

/// Whether `a` and `b` are one file: the same device and inode on Unix, the same volume serial
/// number and file index on Windows. False when either is a symbolic link, a junction or any
/// other reparse point. Err when either cannot be read (a missing file included).
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
    .map_err(io::Error::from)?;
    // SAFETY: `raw` is a valid handle CreateFileW just returned, owned by nothing else; the
    // `OwnedHandle` closes it once, when dropped.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is open for the call's duration, and `info` is a valid, writable
    // BY_HANDLE_FILE_INFORMATION.
    unsafe { GetFileInformationByHandle(HANDLE(handle.as_raw_handle()), &raw mut info) }
        .map_err(io::Error::from)?;
    drop(handle);
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Ok(None);
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    let id = (u64::from(info.dwVolumeSerialNumber), index);
    Ok(Some((id, u64::from(info.nNumberOfLinks))))
}
