//! The refresh's reads of the install folder (step 5), which the lawyer controls and may
//! re-point through a junction at any time. Nothing here is opened by its full path: the
//! drive's root is opened, then each folder of `UserInstallDir` relative to its parent's handle
//! (`NtCreateFile` with `RootDirectory`, `OBJ_DONT_REPARSE` and `FILE_OPEN_REPARSE_POINT`), each
//! refused if a reparse point, then the exe and the `.sig` relative to the last folder's handle,
//! each checked on its own handle (`source::checked`) and read from it. Every open asks for
//! reading alone; SYSTEM never writes or deletes in the folder.
#![expect(unsafe_code, reason = "FFI to CreateFileW and NtCreateFile")]

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, Prefix};

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
    FILE_SEQUENTIAL_ONLY, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, HANDLE, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
    RtlNtStatusToDosError, UNICODE_STRING,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ACCESS_RIGHTS, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FileAttributeTagInfo,
    GetFileInformationByHandleEx, OPEN_EXISTING, SYNCHRONIZE,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::core::{HSTRING, PWSTR};

use super::source::{Handle, checked, code_of, read_all};
use super::windows_refresher::{EXE_NAME, SIG_NAME};
use super::{EXE_CAP, SIG_CAP, SourceError, SourceFile, Sources};

/// `ERROR_REPARSE_POINT_ENCOUNTERED`: what `STATUS_REPARSE_POINT_ENCOUNTERED` maps to.
const REPARSE_ENCOUNTERED: u32 = 4395;

/// The exe and `.sig` of `dir`, read as the module says.
pub(super) fn read_sources(dir: &Path) -> Result<Sources, (SourceFile, SourceError)> {
    let folder = open_chain(dir).map_err(|error| (SourceFile::Folder, error))?;
    let exe = read_child(&folder, EXE_NAME, EXE_CAP).map_err(|error| (SourceFile::Exe, error))?;
    let sig = read_child(&folder, SIG_NAME, SIG_CAP).map_err(|error| (SourceFile::Sig, error))?;
    Ok(Sources { exe, sig })
}

/// `dir`'s last folder, every folder from the drive's root opened relative to its parent and
/// refused if a reparse point (the root included).
pub(super) fn open_chain(dir: &Path) -> Result<Handle, SourceError> {
    let mut parts = dir.components();
    // Only a plain drive path (`C:\...`), as the record's checks already hold it to.
    let Some(Component::Prefix(prefix)) = parts.next() else {
        return Err(SourceError::NotRegular);
    };
    let Prefix::Disk(letter) = prefix.kind() else {
        return Err(SourceError::NotRegular);
    };
    let drive = char::from(letter);
    if parts.next() != Some(Component::RootDir) {
        return Err(SourceError::NotRegular);
    }
    let mut folder = open_root(drive)?;
    for part in parts {
        let Component::Normal(name) = part else {
            return Err(SourceError::NotRegular);
        };
        folder = open_at(&folder, name, Kind::Folder)?;
        folder_checked(&folder)?;
    }
    Ok(folder)
}

/// `name` in `folder`, checked and read, at most `cap` bytes.
fn read_child(folder: &Handle, name: &str, cap: u64) -> Result<Vec<u8>, SourceError> {
    let file = open_at(folder, OsStr::new(name), Kind::File)?;
    if checked(&file)? > cap {
        return Err(SourceError::TooLarge(cap));
    }
    read_all(&file, cap)
}

/// The drive `letter`'s root folder, opened for traversal, not followed if a reparse point.
fn open_root(letter: char) -> Result<Handle, SourceError> {
    let name = HSTRING::from(format!(r"\\?\{letter}:\"));
    // SAFETY: `name` is NUL-terminated and outlives the call; the handle is owned at once.
    let root = unsafe {
        CreateFileW(
            &name,
            (FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map(Handle)
    .map_err(|error| SourceError::Os {
        call: "CreateFileW",
        code: code_of(&error),
    })?;
    folder_checked(&root)?;
    Ok(root)
}

/// What [`open_at`] opens.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A folder, for traversal.
    Folder,
    /// A file, for reading, shared for reading only.
    File,
}

/// `name` (one path part) in `parent`, relative to its handle: `OBJ_DONT_REPARSE` fails the
/// open on any reparse point met, and `FILE_OPEN_REPARSE_POINT` opens one at the name itself
/// rather than what it names, which the caller then refuses.
fn open_at(parent: &Handle, name: &OsStr, kind: Kind) -> Result<Handle, SourceError> {
    let units: Vec<u16> = name.encode_wide().collect();
    let bytes = u16::try_from(units.len() * 2).map_err(|_too_long| SourceError::NotRegular)?;
    let unicode = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(units.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
        RootDirectory: parent.0,
        ObjectName: &raw const unicode,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..OBJECT_ATTRIBUTES::default()
    };
    let (access, share, options) = match kind {
        Kind::Folder => (
            FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_DIRECTORY_FILE,
        ),
        Kind::File => (
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_SHARE_READ,
            FILE_NON_DIRECTORY_FILE | FILE_SEQUENTIAL_ONLY,
        ),
    };
    let mut handle = HANDLE::default();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: `unicode` points into `units`, which outlives the call and is never written;
    // `attributes` and the out-pointers are live; the parent's handle is open; the handle made
    // is owned by `Handle` from here.
    let done = unsafe {
        NtCreateFile(
            &raw mut handle,
            FILE_ACCESS_RIGHTS(access.0),
            &raw const attributes,
            &raw mut status,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            share,
            FILE_OPEN,
            options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if done.is_ok() {
        return Ok(Handle(handle));
    }
    // SAFETY: RtlNtStatusToDosError only maps a number.
    let code = unsafe { RtlNtStatusToDosError(done) };
    Err(match code {
        REPARSE_ENCOUNTERED => SourceError::ReparsePoint(0),
        code if code == ERROR_FILE_NOT_FOUND.0 || code == ERROR_PATH_NOT_FOUND.0 => {
            SourceError::Missing
        }
        code => SourceError::Os {
            call: "NtCreateFile",
            code,
        },
    })
}

/// Refuses an open folder that is a reparse point, or isn't a folder.
fn folder_checked(folder: &Handle) -> Result<(), SourceError> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle is open with FILE_READ_ATTRIBUTES and `tag` is valid and writable for
    // its size.
    unsafe {
        GetFileInformationByHandleEx(
            folder.0,
            FileAttributeTagInfo,
            (&raw mut tag).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0),
        )
    }
    .map_err(|error| SourceError::Os {
        call: "GetFileInformationByHandleEx",
        code: code_of(&error),
    })?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 || tag.ReparseTag != 0 {
        return Err(SourceError::ReparsePoint(tag.ReparseTag));
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
        return Err(SourceError::NotRegular);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
