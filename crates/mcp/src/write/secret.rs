//! The proxy's secret file (`browser_guard/env.rs`), taken once through one handle: opened
//! without following a link at its name, checked through that handle to be a plain file with
//! one name, read, emptied and then deleted, so the guard never reads, empties or deletes
//! anything it did not open that way. A link planted at the name, pointing at a file the agent
//! cannot write itself, is refused and its target left as it was (the review of #1093).
//!
//! - Unix: `open(O_RDWR|O_NOFOLLOW|O_NONBLOCK|O_NOCTTY|O_CLOEXEC)` (a symlink fails with
//!   `ELOOP`; `O_NONBLOCK` keeps a FIFO from blocking the open), `fstat` for the type and
//!   `st_nlink`, `ftruncate` and `fsync`. Unix has no delete through a handle: the name is
//!   unlinked only while `lstat` of it still shows the opened file (device and inode).
//! - Windows: `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT` (a link is opened itself),
//!   read, write and `DELETE` access and `FILE_SHARE_READ` alone (a file another handle holds
//!   without delete or write sharing fails the open, so it is never touched; while the guard
//!   holds it nobody else may write, rename or delete it), `GetFileInformationByHandle` for the
//!   attributes and `nNumberOfLinks`, `SetFileInformationByHandle(FileEndOfFileInfo)` (std's
//!   `set_len`) and `FlushFileBuffers`, then `SetFileInformationByHandle(FileDispositionInfo)`:
//!   the delete goes through the same handle, with no second open of the path.

use std::io::{self, Read};
use std::path::Path;

/// What [`take`] got: the file's first bytes, and why it was not deleted when it was not (it
/// was emptied all the same).
#[derive(Debug)]
pub(crate) struct Taken {
    pub(crate) bytes: Vec<u8>,
    pub(crate) undeleted: Option<io::Error>,
}

/// Why [`take`] failed. Only [`Refused::Unread`] and [`Refused::NotEmptied`] come after the
/// checks passed; before them nothing was changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// A link (or any reparse point), a folder, a device or a file with more than one name.
    NotPlain,
    /// It could not be opened for reading and writing (and on Windows deleting).
    Unopened,
    /// It was opened and checked, but not read; it was emptied all the same.
    Unread,
    /// It was opened, checked and read, but could not be emptied; it is left, not deleted.
    NotEmptied,
}

/// Takes the file at `path`: at most `max` bytes of it are read, then it is emptied through
/// the handle it was read through, then deleted. A delete that fails leaves an empty file and
/// is in [`Taken::undeleted`].
pub(crate) fn take(path: &Path, max: u64) -> Result<Taken, Refused> {
    let file = open_once(path).map_err(|error| {
        if is_link_refusal(&error) {
            Refused::NotPlain
        } else {
            Refused::Unopened
        }
    })?;
    if !is_one_plain_file(&file).map_err(|_unchecked| Refused::Unopened)? {
        return Err(Refused::NotPlain);
    }
    let mut bytes = Vec::new();
    let read = (&file).take(max).read_to_end(&mut bytes);
    file.set_len(0)
        .and_then(|()| file.sync_all())
        .map_err(|_unemptied| Refused::NotEmptied)?;
    if read.is_err() {
        bytes.fill(0);
        return Err(Refused::Unread);
    }
    let undeleted = delete_opened(path, &file).err();
    Ok(Taken { bytes, undeleted })
}

#[cfg(unix)]
fn open_once(path: &Path) -> io::Result<std::fs::File> {
    use nix::libc;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY | libc::O_CLOEXEC)
        .open(path)
}

/// `ELOOP`: `O_NOFOLLOW` met a symlink at the name.
#[cfg(unix)]
fn is_link_refusal(error: &io::Error) -> bool {
    error.raw_os_error() == Some(nix::libc::ELOOP)
}

/// `fstat` of the handle: a regular file with `st_nlink` 1.
#[cfg(unix)]
fn is_one_plain_file(file: &std::fs::File) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let held = file.metadata()?;
    Ok(held.file_type().is_file() && held.nlink() == 1)
}

/// Unlinks `path` only while it still names `file` (`lstat`, never following a link, against
/// the handle's `fstat`); another file swapped in at the name since the open stays.
#[cfg(unix)]
fn delete_opened(path: &Path, file: &std::fs::File) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;
    let held = file.metadata()?;
    let named = std::fs::symlink_metadata(path)?;
    if (named.dev(), named.ino()) != (held.dev(), held.ino()) {
        return Err(io::Error::other("the name no longer holds the file opened"));
    }
    std::fs::remove_file(path)
}

#[cfg(windows)]
fn open_once(path: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        FILE_SHARE_READ,
    };
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .access_mode(FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE.0)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
}

/// Windows opens a link itself: the attribute check refuses it.
#[cfg(windows)]
fn is_link_refusal(_error: &io::Error) -> bool {
    false
}

/// `GetFileInformationByHandle`: neither a folder nor a reparse point, with one link.
#[cfg(windows)]
fn is_one_plain_file(file: &std::fs::File) -> io::Result<bool> {
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    };
    let (attributes, links) = super::windows::attributes_and_links(file)?;
    let refused = FILE_ATTRIBUTE_DIRECTORY.0 | FILE_ATTRIBUTE_REPARSE_POINT.0;
    Ok(attributes & refused == 0 && links == 1)
}

/// The delete through the handle (`FileDispositionInfo`); the name goes when it closes.
#[cfg(windows)]
fn delete_opened(_path: &Path, file: &std::fs::File) -> io::Result<()> {
    super::windows::delete_by_handle(file)
}
