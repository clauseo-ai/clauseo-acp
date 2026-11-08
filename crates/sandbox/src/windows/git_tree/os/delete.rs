//! What the Git trees delete below `git\`, by a plain delete ([`remove_tree`]) or at the next
//! restart ([`delete_at_reboot`]). Each entry is opened relative to its folder's handle as
//! itself (`OBJ_DONT_REPARSE`, `FILE_OPEN_REPARSE_POINT`), a link deleted as itself and never
//! entered. With `SeBackupPrivilege` and `SeRestorePrivilege` and backup intent a DACL that
//! shuts Administrators out still opens for `DELETE`, so nothing's security is ever changed:
//! a tree's files are hard links that share it with the files they name.
#![expect(
    unsafe_code,
    reason = "FFI to SetFileInformationByHandle (the delete) and MoveFileExW (at a restart)"
)]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX,
    FILE_DISPOSITION_INFO_EX_FLAGS, FILE_LIST_DIRECTORY, FileDispositionInfo,
    FileDispositionInfoEx, MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW, SetFileInformationByHandle,
};
use windows::core::PCWSTR;

use crate::windows::acl::object::{Handle, Object, os_error};
use crate::windows::acl::privilege::{BACKUP_RESTORE, Privileges};
use crate::windows::folder::verbatim_wide;

/// What an entry is opened for to be deleted, and listed when a folder.
const REMOVE: u32 = DELETE.0 | FILE_LIST_DIRECTORY.0;

/// The errors that say `FileDispositionInfoEx` isn't supported here (before Windows 10 1709,
/// or a file system without it): `ERROR_INVALID_FUNCTION`, `ERROR_NOT_SUPPORTED`,
/// `ERROR_INVALID_PARAMETER` and `ERROR_INVALID_LEVEL`.
const EX_UNSUPPORTED: [i32; 4] = [1, 50, 87, 124];

/// Deletes `path` and all below it by a plain delete, as the module says: each file and link
/// deleted as itself (POSIX semantics, so its name goes at once), each folder after what it
/// holds. Goes on past what can't be deleted (a loaded image, a file another process holds),
/// then fails with how many were left; missing is `Ok`.
pub(super) fn remove_tree(path: &Path) -> Result<(), String> {
    let fail = |why: &dyn std::fmt::Display| format!("deleting {}: {why}", path.display());
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(fail(&"it has no parent"));
    };
    let _held = Privileges::enable(&BACKUP_RESTORE).map_err(|error| fail(&error))?;
    let above = match Object::open_folder(parent) {
        Ok(above) => above,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(fail(&error)),
    };
    if above.reparse() || !above.folder {
        return Err(fail(&"its parent isn't a plain folder"));
    }
    let name: Vec<u16> = name.encode_wide().collect();
    let target = match above.open_child_for(&name, REMOVE) {
        Ok(target) => target,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(fail(&error)),
    };
    let mut left = 0_usize;
    remove_object(&target, &mut left);
    if left == 0 {
        Ok(())
    } else {
        Err(fail(&format!("{left} entries couldn't be deleted")))
    }
}

/// Deletes what `object` is open on, and first all in it when a plain folder, each child
/// opened relative to its handle; counts in `left` each that couldn't be opened or deleted.
fn remove_object(object: &Object, left: &mut usize) {
    if object.folder && !object.reparse() {
        match object.children() {
            Ok(children) => {
                for child in children {
                    match object.open_child_for(&child.name, REMOVE) {
                        Ok(below) => remove_object(&below, left),
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(_) => *left += 1,
                    }
                }
            }
            Err(_) => *left += 1,
        }
    }
    if let Err(error) = dispose(object.handle()) {
        tracing::debug!(%error, "an entry of a Git tree couldn't be deleted");
        *left += 1;
    }
}

/// Marks what `handle` is open on (with `DELETE`) deleted: `FileDispositionInfoEx` with POSIX
/// semantics, ignoring the read-only attribute, else `FileDispositionInfo` where that isn't
/// supported (the name then goes when the last handle closes).
fn dispose(handle: &Handle) -> io::Result<()> {
    let ex = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_INFO_EX_FLAGS(
            FILE_DISPOSITION_FLAG_DELETE.0
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE.0,
        ),
    };
    let size = u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).unwrap_or(0);
    // SAFETY: the handle is open with DELETE; `ex` is a live FILE_DISPOSITION_INFO_EX of
    // `size` bytes.
    let done = unsafe {
        SetFileInformationByHandle(
            handle.0,
            FileDispositionInfoEx,
            (&raw const ex).cast(),
            size,
        )
    };
    let error = match done {
        Ok(()) => return Ok(()),
        Err(error) => os_error(&error),
    };
    if !error
        .raw_os_error()
        .is_some_and(|code| EX_UNSUPPORTED.contains(&code))
    {
        return Err(error);
    }
    let plain = FILE_DISPOSITION_INFO { DeleteFile: true };
    let size = u32::try_from(size_of::<FILE_DISPOSITION_INFO>()).unwrap_or(0);
    // SAFETY: as above; `plain` is a live FILE_DISPOSITION_INFO of `size` bytes.
    unsafe {
        SetFileInformationByHandle(
            handle.0,
            FileDispositionInfo,
            (&raw const plain).cast(),
            size,
        )
    }
    .map_err(|error| os_error(&error))
}

/// Schedules every file and link below `path` (listed through each folder's handle, a link
/// never entered), then its folders deepest first, then `path`, for deletion at the next
/// restart (`MOVEFILE_DELAY_UNTIL_REBOOT`). Goes on past what fails, then fails; missing is
/// `Ok`.
pub(super) fn delete_at_reboot(path: &Path) -> Result<(), String> {
    let _backup = Privileges::enable(&["SeBackupPrivilege"]).ok();
    let mut files = Vec::new();
    let mut folders = Vec::new();
    let mut left = 0_usize;
    match Object::open_folder(path) {
        Ok(top) if top.folder && !top.reparse() => {
            list(&top, Path::new(""), &mut files, &mut folders, &mut left);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        // Still scheduled itself: listing it is what failed.
        Err(_) => left += 1,
    }
    let below = files.iter().chain(folders.iter().rev());
    for rel in below {
        if schedule(&path.join(rel)).is_err() {
            left += 1;
        }
    }
    if let Err(error) = schedule(path) {
        tracing::debug!(%error, "a Git tree's folder couldn't be scheduled for deletion");
        left += 1;
    }
    if left == 0 {
        Ok(())
    } else {
        Err(format!(
            "scheduling {} for deletion at a restart: {left} entries failed",
            path.display()
        ))
    }
}

/// Lists below the open folder `folder` (`rel` from the root): files and links into `files`,
/// folders into `folders` each before what it holds; counts in `left` what can't be listed.
fn list(
    folder: &Object,
    rel: &Path,
    files: &mut Vec<PathBuf>,
    folders: &mut Vec<PathBuf>,
    left: &mut usize,
) {
    let Ok(children) = folder.children() else {
        *left += 1;
        return;
    };
    for child in children {
        let path = rel.join(OsString::from_wide(&child.name));
        if !child.folder || child.reparse {
            files.push(path);
            continue;
        }
        match folder.open_child_for(&child.name, FILE_LIST_DIRECTORY.0) {
            Ok(below) if below.folder && !below.reparse() => {
                folders.push(path.clone());
                list(&below, &path, files, folders, left);
            }
            Ok(_) => files.push(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => {
                // Not listed, but still scheduled: it may be empty by then.
                folders.push(path);
                *left += 1;
            }
        }
    }
}

/// Schedules `path` for deletion at the next restart (`\\?\` form).
fn schedule(path: &Path) -> io::Result<()> {
    let wide = verbatim_wide(path)?;
    // SAFETY: `wide` is NUL-terminated and outlives the call; no new name means delete.
    unsafe {
        MoveFileExW(
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
    }
    .map_err(|error| os_error(&error))
}
