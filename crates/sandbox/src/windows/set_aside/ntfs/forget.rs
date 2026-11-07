//! A matter's set-aside bookkeeping deleted on NTFS (`set_aside/bookkeeping.rs`), by handle and
//! never through a link. The set-aside folder is reached one of two ways: for `--remove`,
//! `<data_dir>\sandbox` opened by its path as itself and `set-aside` relative to it
//! ([`folder_chain`], each refused if a reparse point); for the uninstaller, whose walk holds
//! `sandbox` open sharing reads only, `set-aside` opened by its path as itself for traversal
//! alone and refused if a reparse point. Then:
//!
//! - the matter's folder, relative to it (`FILE_DIRECTORY_FILE`, with `DELETE`), refused if a
//!   reparse point;
//! - each bookkeeping name relative to the matter's folder with `DELETE`
//!   (`FILE_NON_DIRECTORY_FILE`, so a folder or a junction under the name fails to open), refused
//!   if a reparse point (a file symbolic link opens as itself); one with another hard link
//!   (`FileStandardInfo`) leaves everything as it is. The record is read on that same handle
//!   first, and one that still lists an open session, or can't be read or parsed, leaves
//!   everything as it is too. Each is marked for deletion on its own handle
//!   (`FileDispositionInfo`);
//! - last, the matter's folder is marked on its own handle, which NTFS refuses while anything is
//!   in it (`ERROR_DIR_NOT_EMPTY`): then it stays.
//!
//! Every open is `OBJ_DONT_REPARSE` (`open_at`), so a reparse point met on the way fails it.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::Path;

use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_DISPOSITION_INFO, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_STANDARD_INFO,
    FileDispositionInfo, FileStandardInfo, GetFileInformationByHandleEx, ReadFile, SYNCHRONIZE,
    SetFileInformationByHandle,
};

use super::super::bookkeeping::{RECORD_CAP, SetAsideBookkeeping, set_aside_record_closed};
use super::super::{SET_ASIDE_DIR, SetAsideFailure};
use super::{
    Handle, PARENT, failure, folder_chain, io_failure, open_at, open_path, os_error, reparse,
    wide_os,
};

/// `ERROR_DIR_NOT_EMPTY`: a folder marked for deletion still holds something.
const DIR_NOT_EMPTY: i32 = 145;

/// What a bookkeeping file is opened with: read (the record), deleted, its attributes read.
const FILE: u32 = FILE_READ_DATA.0 | DELETE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;

/// `--remove`'s way: `set-aside` reached from `sandbox`, then [`forget_in`].
pub(in crate::windows::set_aside) fn forget_from_sandbox(
    sandbox: &Path,
    matter: &OsStr,
    names: &[&str],
) -> Result<SetAsideBookkeeping, SetAsideFailure> {
    match folder_chain(sandbox, &[OsString::from(SET_ASIDE_DIR)], FILE_OPEN) {
        Ok(set_aside) => forget_in(&set_aside, matter, names),
        Err(SetAsideFailure::Io(io::ErrorKind::NotFound)) => Ok(SetAsideBookkeeping::NoFolder),
        Err(refused) => Err(refused),
    }
}

/// The uninstaller's way: `set_aside` opened by its path as itself, then [`forget_in`].
pub(in crate::windows::set_aside) fn forget_from_set_aside(
    set_aside: &Path,
    matter: &OsStr,
    names: &[&str],
) -> Result<SetAsideBookkeeping, SetAsideFailure> {
    let folder = match open_path(set_aside, PARENT) {
        Ok(folder) => folder,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(SetAsideBookkeeping::NoFolder);
        }
        Err(error) => return Err(failure(&error)),
    };
    refuse_reparse(&folder)?;
    forget_in(&folder, matter, names)
}

/// Deletes each of `names` in `set_aside\<matter>` that is a plain file with one link, then that
/// folder when it is empty, as the module says. `names[0]` is the record.
fn forget_in(
    set_aside: &Handle,
    matter: &OsStr,
    names: &[&str],
) -> Result<SetAsideBookkeeping, SetAsideFailure> {
    let options = FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT;
    let folder = match open_at(
        set_aside,
        &wide_os(matter),
        PARENT | DELETE.0,
        FILE_OPEN,
        options,
    ) {
        Ok(folder) => folder,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(SetAsideBookkeeping::NoFolder);
        }
        Err(error) => return Err(failure(&error)),
    };
    refuse_reparse(&folder)?;
    let options = FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT;
    for (at, name) in names.iter().enumerate() {
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
        if links(&file).map_err(|error| io_failure(&error))? > 1 {
            return Ok(SetAsideBookkeeping::Left);
        }
        // An unreadable record, one over the cap or one still open keeps everything.
        if at == 0 && !read_capped(&file).is_ok_and(|bytes| set_aside_record_closed(&bytes)) {
            return Ok(SetAsideBookkeeping::Left);
        }
        // Closing the handle at the end of this pass completes the delete.
        mark_deleted(&file).map_err(|error| io_failure(&error))?;
    }
    match mark_deleted(&folder) {
        Ok(()) => Ok(SetAsideBookkeeping::Gone),
        Err(error) if error.raw_os_error() == Some(DIR_NOT_EMPTY) => Ok(SetAsideBookkeeping::Left),
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

/// How many hard links the file `handle` is open on has (`FileStandardInfo`).
fn links(handle: &Handle) -> io::Result<u32> {
    let mut standard = FILE_STANDARD_INFO::default();
    let len = u32::try_from(size_of::<FILE_STANDARD_INFO>()).map_err(io::Error::other)?;
    // SAFETY: the handle is open and `standard` is valid and writable for `len` bytes.
    unsafe {
        GetFileInformationByHandleEx(handle.0, FileStandardInfo, (&raw mut standard).cast(), len)
    }
    .map_err(|error| os_error(&error))?;
    Ok(standard.NumberOfLinks)
}

/// The whole file `handle` is open on (synchronously, with `FILE_READ_DATA`); an error when it
/// holds more than [`RECORD_CAP`] bytes.
fn read_capped(handle: &Handle) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = vec![0_u8; 64 << 10];
    loop {
        let mut read = 0_u32;
        // SAFETY: the handle is open for synchronous reads; `chunk` is a live, writable slice
        // and `read` a live out-pointer.
        unsafe { ReadFile(handle.0, Some(&mut chunk), Some(&raw mut read), None) }
            .map_err(|error| os_error(&error))?;
        let read = usize::try_from(read).map_err(io::Error::other)?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(chunk.get(..read).unwrap_or_default());
        if bytes.len() > RECORD_CAP {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "larger than its cap",
            ));
        }
    }
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
