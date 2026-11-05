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
