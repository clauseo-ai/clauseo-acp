//! A forgotten matter's set-aside bookkeeping deleted on NTFS (`set_aside/bookkeeping.rs`), by
//! handle and never through a link: `<data_dir>\sandbox` is opened by its path as itself and
//! `set-aside` relative to it ([`folder_chain`], each refused if a reparse point); the matter's
//! folder relative to that (`FILE_DIRECTORY_FILE`, with `DELETE`), refused if a reparse point;
//! each bookkeeping name relative to the matter's folder with `DELETE` (`FILE_NON_DIRECTORY_FILE`,
//! so a folder or a junction under the name fails to open), refused if a reparse point (a file
//! symbolic link opens as itself), then marked for deletion on that same handle
//! (`FileDispositionInfo`). Last, the matter's folder is marked on its own handle, which NTFS
//! refuses while anything is in it (`ERROR_DIR_NOT_EMPTY`): then it stays. Every open is
//! `OBJ_DONT_REPARSE` (`open_at`), so a reparse point met on the way fails it.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;

use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_DISPOSITION_INFO, FILE_READ_ATTRIBUTES, FileDispositionInfo, SYNCHRONIZE,
    SetFileInformationByHandle,
};

use super::super::{SET_ASIDE_DIR, SetAsideFailure};
use super::{
    FOLDER, Handle, failure, folder_chain, io_failure, open_at, os_error, reparse, wide_os,
};

/// `ERROR_DIR_NOT_EMPTY`: a folder marked for deletion still holds something.
const DIR_NOT_EMPTY: i32 = 145;

/// What a bookkeeping file is opened with: deleted, its attributes read.
const FILE: u32 = DELETE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;

/// Deletes each of `names` in `sandbox\set-aside\<matter>` that is a plain file, then that folder
/// when it is empty, as the module says; answers whether the folder went.
pub(in crate::windows::set_aside) fn forget_bookkeeping(
    sandbox: &Path,
    matter: &OsStr,
    names: &[&str],
) -> Result<bool, SetAsideFailure> {
    let set_aside = match folder_chain(sandbox, &[OsString::from(SET_ASIDE_DIR)], FILE_OPEN) {
        Ok(folder) => folder,
        Err(SetAsideFailure::Io(io::ErrorKind::NotFound)) => return Ok(false),
        Err(refused) => return Err(refused),
    };
    let options = FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT;
    let folder = match open_at(
        &set_aside,
        &wide_os(matter),
        FOLDER | DELETE.0,
        FILE_OPEN,
        options,
    ) {
        Ok(folder) => folder,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(failure(&error)),
    };
    refuse_reparse(&folder)?;
    let options = FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT;
    for name in names {
        let file = match open_at(
            &folder,
            &wide_os(OsStr::new(name)),
            FILE,
            FILE_OPEN,
            options,
        ) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(failure(&error)),
        };
        refuse_reparse(&file)?;
        // Closing the handle at the end of this pass completes the delete.
        mark_deleted(&file).map_err(|error| io_failure(&error))?;
    }
    match mark_deleted(&folder) {
        Ok(()) => Ok(true),
        Err(error) if error.raw_os_error() == Some(DIR_NOT_EMPTY) => Ok(false),
        Err(error) => Err(io_failure(&error)),
    }
}

/// [`SetAsideFailure::Changed`] when `handle` is open on a reparse point.
fn refuse_reparse(handle: &Handle) -> Result<(), SetAsideFailure> {
    if reparse(handle).map_err(|error| io_failure(&error))? {
        tracing::warn!("a set-aside name is a link; nothing is deleted through it");
        return Err(SetAsideFailure::Changed);
    }
    Ok(())
}

/// Marks what `handle` is open on (with `DELETE`) for deletion when its last handle closes.
fn mark_deleted(handle: &Handle) -> io::Result<()> {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    let len = u32::try_from(size_of::<FILE_DISPOSITION_INFO>()).map_err(io::Error::other)?;
    // SAFETY: the handle is open with `DELETE`; `info` is a live FILE_DISPOSITION_INFO of `len`
    // bytes.
    unsafe {
        SetFileInformationByHandle(handle.0, FileDispositionInfo, (&raw const info).cast(), len)
    }
    .map_err(|error| os_error(&error))
}
