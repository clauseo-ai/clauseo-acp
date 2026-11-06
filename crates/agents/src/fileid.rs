//! File identity: whether two paths name one file, read from the file system's own id and never
//! by opening either file for its contents. Codex's reused login is one file under two names
//! (a hard link), and the guard and the split rule compare those names with [`same_file`].
//!
//! Links are not followed: a symbolic link or junction in either place compares as its own
//! entry, so it is never "the same file" as its target, nor as anything else.

use std::io;
use std::path::Path;

/// Whether `a` and `b` are one file: the same device and inode on Unix, the same volume serial
/// number and file index on Windows. False when either is a symbolic link, a junction or any
/// other reparse point, even if the ids happen to agree. Err when either cannot be read (a
/// missing file included).
///
/// Unix reads `symlink_metadata` (`lstat`), which opens nothing. Windows has no such call in
/// `std` (its `symlink_metadata` itself opens a handle with access 0, as below), so it opens
/// a handle to each path with `CreateFileW` and desired access 0 (no read, write or execute
/// right: the handle can query attributes and ids but cannot read a byte of
/// contents), sharing read, write and delete so no other program is blocked, with
/// `FILE_FLAG_OPEN_REPARSE_POINT` so a link is opened as itself, never followed. The handle is
/// closed before this returns. Opening that handle is part of S18's security review.
pub fn same_file(a: &Path, b: &Path) -> io::Result<bool> {
    let (Some(a), Some(b)) = (file_id(a)?, file_id(b)?) else {
        return Ok(false);
    };
    Ok(a.id == b.id)
}

/// A file's id, last write, last status change and size, read as [`same_file`] reads ids, from
/// metadata alone (never the contents). R17's identity check caches its verdict on a login file
/// by this, and checks again once it changes (another file under the name, or the same file
/// written, even with its last write put back).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    /// (device, inode) on Unix; (volume serial number, file index) on Windows.
    id: (u64, u64),
    /// The last write: seconds and nanoseconds since the epoch on Unix; 100 ns ticks since
    /// 1601 on Windows.
    modified: (i64, i64),
    /// The last status change, in the same units as `modified`: ctime on Unix, which every
    /// write moves whatever the last write says; `ChangeTime` on Windows.
    changed: (i64, i64),
    /// The length in bytes.
    size: u64,
}

/// `path`'s [`FileStamp`], never following a link; `None` for a symbolic link, a junction or
/// any other reparse point. Err when it cannot be read (a missing file included). It reads the
/// id, last write, last status change and size from metadata, never the contents: no digest or
/// hash of the bytes. Opens nothing on Unix; on Windows the same access-0 handle as
/// [`same_file`].
pub fn file_stamp(path: &Path) -> io::Result<Option<FileStamp>> {
    file_id(path)
}

/// The file's [`FileStamp`]: id (device, inode), last write, last status change (ctime) and
/// size. `None` for a symbolic link.
#[cfg(unix)]
fn file_id(path: &Path) -> io::Result<Option<FileStamp>> {
    use std::os::unix::fs::MetadataExt as _;

    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(None);
    }
    Ok(Some(FileStamp {
        id: (meta.dev(), meta.ino()),
        modified: (meta.mtime(), meta.mtime_nsec()),
        changed: (meta.ctime(), meta.ctime_nsec()),
        size: meta.len(),
    }))
}

/// The file's [`FileStamp`]: id (volume serial number, file index), last write, `ChangeTime` and
/// size. `None` for a reparse point (a symbolic link, a junction, or any other).
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "CreateFileW and GetFileInformationByHandle are the OS's file-id calls; std has no stable one"
)]
fn file_id(path: &Path) -> io::Result<Option<FileStamp>> {
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_BASIC_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FileBasicInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx, OPEN_EXISTING,
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
    // ChangeTime can itself be set through SetFileInformationByHandle (FileBasicInfo), so a
    // write that restores all of its times can go unseen here. The size still catches a write
    // that changes the length.
    let mut basic = FILE_BASIC_INFO::default();
    let basic_len =
        u32::try_from(std::mem::size_of::<FILE_BASIC_INFO>()).map_err(io::Error::other)?;
    // SAFETY: `handle` is open for the call's duration, and `basic` is a valid, writable
    // FILE_BASIC_INFO of `basic_len` bytes, the size of the class FileBasicInfo returns.
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(handle.as_raw_handle()),
            FileBasicInfo,
            (&raw mut basic).cast(),
            basic_len,
        )
    }
    .map_err(io::Error::from)?;
    drop(handle);
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Ok(None);
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    let written = info.ftLastWriteTime;
    let ticks = (u64::from(written.dwHighDateTime) << 32) | u64::from(written.dwLowDateTime);
    let size = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
    Ok(Some(FileStamp {
        id: (u64::from(info.dwVolumeSerialNumber), index),
        modified: (i64::try_from(ticks).unwrap_or(i64::MAX), 0),
        changed: (basic.ChangeTime, 0),
        size,
    }))
}

#[cfg(test)]
mod tests;
