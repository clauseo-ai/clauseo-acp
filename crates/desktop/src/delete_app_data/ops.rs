//! The deletion's only changes to the file system, and the handle that pins a folder while it is
//! emptied. The desktop crate has no write module; these wrappers are the one place
//! `--delete-app-data` removes anything.
#![expect(
    clippy::disallowed_methods,
    reason = "the uninstaller's deletion of the data folders: these wrappers are its only writes"
)]

use std::fs::Metadata;
use std::io;
use std::path::Path;

/// A folder held open by handle while it is emptied ([`pin_folder`]).
pub(super) struct Pinned {
    /// The open folder; closing it unpins.
    #[cfg(windows)]
    _handle: std::fs::File,
}

/// Opens the folder `path` by handle without following a reparse point
/// (`FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS`), and refuses it unless that
/// handle's own attributes (`GetFileInformationByHandle`, std's `File::metadata`) say a folder
/// and no reparse point. It shares read access only, so while it is held no one can open the
/// folder to delete, rename or write it (turning it into a junction needs a write handle), and
/// Windows refuses to rename a folder above it: its path keeps naming this folder.
#[cfg(windows)]
pub(super) fn pin_folder(path: &Path) -> io::Result<Pinned> {
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    /// `FILE_LIST_DIRECTORY`.
    const FILE_LIST_DIRECTORY: u32 = 0x1;
    /// `FILE_READ_ATTRIBUTES`.
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    /// `FILE_SHARE_READ`.
    const FILE_SHARE_READ: u32 = 0x1;
    /// `FILE_FLAG_BACKUP_SEMANTICS`: needed to open a folder.
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    /// `FILE_FLAG_OPEN_REPARSE_POINT`: open a link itself, never its target.
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    /// `FILE_ATTRIBUTE_DIRECTORY`.
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    let handle = std::fs::OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let attributes = handle.metadata()?.file_attributes();
    if attributes & super::walk::FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::other(
            "it is a reparse point (a junction or a link); not entered",
        ));
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::other("it is not a folder; not entered"));
    }
    Ok(Pinned { _handle: handle })
}

/// The tests' stand-in off Windows: `path` must be a folder and no symbolic link. Nothing is
/// held, and nothing races in a test.
#[cfg(not(windows))]
pub(super) fn pin_folder(path: &Path) -> io::Result<Pinned> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::other(
            "it is a reparse point (a junction or a link); not entered",
        ));
    }
    if !meta.is_dir() {
        return Err(io::Error::other("it is not a folder; not entered"));
    }
    Ok(Pinned {})
}

/// Removes the reparse point `path` as a link, never what it points at: `remove_dir` when the
/// link itself is a directory one (a junction, a directory symbolic link, a mount point), else
/// `remove_file`. `meta` is `path`'s own, from `symlink_metadata`.
#[cfg(windows)]
pub(super) fn remove_link(path: &Path, meta: &Metadata) -> io::Result<()> {
    use std::os::windows::fs::MetadataExt as _;
    /// `FILE_ATTRIBUTE_DIRECTORY`.
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    if meta.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0 {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Removes the symbolic link `path` (`unlink`), never what it points at.
#[cfg(not(windows))]
pub(super) fn remove_link(path: &Path, _meta: &Metadata) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// `IO_REPARSE_TAG_MOUNT_POINT`: a directory junction (or a volume mount point).
pub(super) const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
/// `IO_REPARSE_TAG_SYMLINK`: a symbolic link, to a folder or a file.
pub(super) const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
/// The bit `IsReparseTagNameSurrogate` tests: the reparse point names another file, so deleting
/// it removes only the name.
pub(super) const NAME_SURROGATE_BIT: u32 = 0x2000_0000;

/// What a `sandbox` listed as a reparse point turned out to be, read on the handle that would
/// delete it ([`unlink_name_surrogate`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LinkVerdict {
    /// No reparse point at all (a real folder put back after it was listed): kept, never
    /// deleted here.
    NotALink,
    /// A junction or a symbolic link, both name surrogates: removed, the link alone.
    Unlinked,
    /// Any other reparse point (a cloud placeholder, a deduplicated file), with its tag: kept,
    /// never deleted.
    Kept(u32),
}

/// The decision behind [`unlink_name_surrogate`], from the handle's own attributes: whether it is
/// a reparse point at all, and its tag. Only a junction or a symbolic link is unlinked, each a
/// name surrogate; any other tag, a name surrogate or not, is kept.
pub(super) fn link_verdict(reparse_point: bool, tag: u32) -> LinkVerdict {
    let link = tag == IO_REPARSE_TAG_MOUNT_POINT || tag == IO_REPARSE_TAG_SYMLINK;
    match (reparse_point, link && tag & NAME_SURROGATE_BIT != 0) {
        (false, _) => LinkVerdict::NotALink,
        (true, true) => LinkVerdict::Unlinked,
        (true, false) => LinkVerdict::Kept(tag),
    }
}

/// Removes the link at `path` (the data folder's `sandbox`, listed as a reparse point) by handle,
/// never what it points at, and answers what it was. It opens `path` itself
/// (`FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS`) with `DELETE` access, reads the
/// attributes and the reparse tag on that handle (`GetFileInformationByHandleEx`,
/// `FileAttributeTagInfo`), and only for a junction or a symbolic link ([`link_verdict`]) marks
/// that same handle for deletion (`SetFileInformationByHandle`: `FileDispositionInfoEx` with
/// `FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS`, or
/// `FileDispositionInfo` where the file system has no `Ex`). What is checked is what is deleted:
/// a real folder swapped in after the listing opens as no reparse point and is left alone, and a
/// handle's delete never recurses.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "FFI: the reparse tag and the delete on one handle"
)]
pub(super) fn unlink_name_surrogate(path: &Path) -> io::Result<LinkVerdict> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use std::os::windows::io::AsRawHandle as _;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_TAG_INFO, FILE_DISPOSITION_FLAG_DELETE,
        FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_DISPOSITION_INFO_EX_FLAGS,
        FileAttributeTagInfo, FileDispositionInfo, FileDispositionInfoEx,
        GetFileInformationByHandleEx, SetFileInformationByHandle,
    };
    use windows::core::HRESULT;
    /// `DELETE`.
    const DELETE: u32 = 0x0001_0000;
    /// `FILE_READ_ATTRIBUTES`.
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    /// `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`: the delete is bound to this
    /// handle's file whatever else holds it, so nothing needs to be shut out.
    const FILE_SHARE_ALL: u32 = 0x7;
    /// `FILE_FLAG_BACKUP_SEMANTICS`: needed to open a folder.
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    /// `FILE_FLAG_OPEN_REPARSE_POINT`: open a link itself, never its target.
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    /// `ERROR_INVALID_FUNCTION`, `ERROR_NOT_SUPPORTED`, `ERROR_INVALID_PARAMETER`: the file
    /// system or the Windows version has no `FileDispositionInfoEx`.
    const NO_EX: [u32; 3] = [1, 50, 87];

    let file = std::fs::OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_ALL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let handle = HANDLE(file.as_raw_handle());
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    let len = u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).map_err(io::Error::other)?;
    // SAFETY: `handle` is open for the call (`file` lives to the end of this function); `info`
    // is a live, writable FILE_ATTRIBUTE_TAG_INFO of `len` bytes.
    unsafe {
        GetFileInformationByHandleEx(handle, FileAttributeTagInfo, (&raw mut info).cast(), len)
    }
    .map_err(|error| os_error(&error))?;
    let reparse_point = info.FileAttributes & super::walk::FILE_ATTRIBUTE_REPARSE_POINT != 0;
    let verdict = link_verdict(reparse_point, info.ReparseTag);
    if verdict != LinkVerdict::Unlinked {
        return Ok(verdict);
    }
    let flags = FILE_DISPOSITION_FLAG_DELETE.0
        | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0
        | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE.0;
    let ex = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_INFO_EX_FLAGS(flags),
    };
    let len = u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).map_err(io::Error::other)?;
    // SAFETY: `handle` is open with DELETE access; `ex` is a live FILE_DISPOSITION_INFO_EX of
    // `len` bytes.
    let marked = unsafe {
        SetFileInformationByHandle(handle, FileDispositionInfoEx, (&raw const ex).cast(), len)
    };
    match marked {
        Ok(()) => {}
        // `FileDispositionInfo` has no flag to ignore the read-only attribute, so this fallback
        // cannot remove a read-only link: it fails, and the link is reported and kept (held; no
        // attribute is changed).
        Err(error)
            if NO_EX
                .iter()
                .any(|&code| error.code() == HRESULT::from_win32(code)) =>
        {
            let plain = FILE_DISPOSITION_INFO { DeleteFile: true };
            let len =
                u32::try_from(size_of::<FILE_DISPOSITION_INFO>()).map_err(io::Error::other)?;
            // SAFETY: as above, with a live FILE_DISPOSITION_INFO of `len` bytes.
            unsafe {
                SetFileInformationByHandle(
                    handle,
                    FileDispositionInfo,
                    (&raw const plain).cast(),
                    len,
                )
            }
            .map_err(|error| os_error(&error))?;
        }
        Err(error) => return Err(os_error(&error)),
    }
    // Closing the handle completes the delete of the link (at once with POSIX semantics).
    drop(file);
    Ok(verdict)
}

/// `error` as the Win32 error it carries (`HRESULT_FROM_WIN32`), so the line says what std's
/// own errors say; any other code with its message.
#[cfg(windows)]
fn os_error(error: &windows::core::Error) -> io::Error {
    let code = error.code().0.cast_unsigned();
    match u16::try_from(code & 0xFFFF) {
        Ok(win32) if code & 0xFFFF_0000 == 0x8007_0000 => {
            io::Error::from_raw_os_error(i32::from(win32))
        }
        _ => io::Error::other(error.message()),
    }
}

/// The tests' stand-in off Windows: a symbolic link (tagged as Windows tags one) is removed as a
/// link, anything else is left alone.
#[cfg(not(windows))]
pub(super) fn unlink_name_surrogate(path: &Path) -> io::Result<LinkVerdict> {
    let meta = std::fs::symlink_metadata(path)?;
    let verdict = link_verdict(meta.file_type().is_symlink(), IO_REPARSE_TAG_SYMLINK);
    if verdict == LinkVerdict::Unlinked {
        std::fs::remove_file(path)?;
    }
    Ok(verdict)
}

/// Removes the file `path`. On Windows std removes a read-only file too (Rust 1.99's `unlink`
/// retries with `FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE`), so no attribute is changed:
/// a hard link's attribute is the lawyer's file's too.
pub(super) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Removes the folder `path` and everything in it with std, which opens each entry by handle
/// without following a reparse point and removes read-only entries.
pub(super) fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}
