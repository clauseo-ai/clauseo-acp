//! The admin folder's tree lock-down on NTFS ([`lock_tree`]), walked through handles so no
//! name is looked up twice and no link is followed: the folder is opened once as itself
//! (`FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS`, `acl::object::Object`), refused
//! if a reparse point, locked and listed through that handle (`FileIdBothDirectoryInfo`); each
//! entry is opened relative to it (`NtCreateFile` with `RootDirectory`, `OBJ_DONT_REPARSE` and
//! `FILE_OPEN_REPARSE_POINT`), and a link below is deleted as itself, through its own handle.
//! A file with more than one name is refused, not locked (`os::lock_handle`). A take-back
//! ([`take_back`]) opens what it takes back the same way, a file as much as a folder.
#![expect(unsafe_code, reason = "FFI to SetFileInformationByHandle")]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt as _;
use std::path::Path;

use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
};

use super::os::lock_handle;
use crate::windows::acl::object::{Child, Object, os_error};
use crate::windows::sids::Sid;

/// [`os::lock_down`](super::os::lock_down) for `group` on `dir` and, below it, on every folder
/// and file but `skip` (a name in `dir`); a link below is deleted, `dir` being one is refused.
pub(crate) fn lock_tree(dir: &Path, skip: Option<&str>, group: Option<&Sid>) -> io::Result<()> {
    let root = Object::open_folder(dir)?;
    if root.reparse() || !root.folder {
        return Err(io::Error::other(
            "the admin folder isn't a plain folder; left as it is",
        ));
    }
    lock_level(&root, skip, group)
}

/// A take-back's lock-down of `path` (`take_back_os`), opened once as itself and refused
/// through no link: a folder is locked with all below it, as [`lock_tree`] does; a file is
/// locked and read back through that same handle, and refused if it has more than one name. A
/// link, or nothing there, is left: a link is deleted as itself, never taken back through.
/// Locked for `group`, as [`lock_tree`].
pub(crate) fn take_back(path: &Path, group: Option<&Sid>) -> io::Result<()> {
    // The listing right is `FILE_READ_DATA` on a file, which backup semantics grants.
    let object = match Object::open_folder(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        other => other?,
    };
    if object.reparse() {
        Ok(())
    } else if object.folder {
        lock_level(&object, None, group)
    } else {
        lock(&object, group)
    }
}

/// Locks the open folder `folder`, then each entry it lists but `skip`, as the module says.
fn lock_level(folder: &Object, skip: Option<&str>, group: Option<&Sid>) -> io::Result<()> {
    lock(folder, group)?;
    for child in folder.children()? {
        let name = OsString::from_wide(&child.name);
        if skip.is_some_and(|skip| name.eq_ignore_ascii_case(skip)) {
            continue;
        }
        if child.reparse {
            delete_link(folder, &child)?;
            continue;
        }
        let object = folder.open_child(&child.name, child.folder)?;
        if object.reparse() {
            // Made a link after the listing: deleted as itself all the same.
            drop(object);
            delete_link(folder, &child)?;
        } else if object.folder != child.folder {
            return Err(io::Error::other(
                "an entry in the admin folder changed while it was locked",
            ));
        } else if object.folder {
            lock_level(&object, None, group)?;
        } else {
            lock(&object, group)?;
        }
    }
    Ok(())
}

/// [`lock_handle`] for `group` on `object`, which must have been opened with `WRITE_DAC` and
/// `WRITE_OWNER`; without them it is access denied, which a take-back answers.
fn lock(object: &Object, group: Option<&Sid>) -> io::Result<()> {
    if !(object.write_dac && object.write_owner) {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(ERROR_ACCESS_DENIED.0).unwrap_or(5),
        ));
    }
    lock_handle(object.handle().0, object.folder, group)
}

/// Deletes the link `child` in `folder` as itself: opened relative to the folder, never
/// followed, and marked for deletion on its own handle (`FileDispositionInfo`).
fn delete_link(folder: &Object, child: &Child) -> io::Result<()> {
    let link = folder.open_child_for(&child.name, DELETE.0)?;
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    let len = u32::try_from(size_of::<FILE_DISPOSITION_INFO>()).map_err(io::Error::other)?;
    // SAFETY: the handle is open with `DELETE`; `info` is a live FILE_DISPOSITION_INFO of `len`
    // bytes.
    unsafe {
        SetFileInformationByHandle(
            link.handle().0,
            FileDispositionInfo,
            (&raw const info).cast(),
            len,
        )
    }
    .map_err(|error| os_error(&error))
}
