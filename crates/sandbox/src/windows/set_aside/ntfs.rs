//! The set-aside's file-system calls on NTFS (S15 PR3, M6). Nothing here follows a link:
//!
//! - A file ID is read from a handle opened on the name itself (`FILE_FLAG_OPEN_REPARSE_POINT`,
//!   `FILE_READ_ATTRIBUTES` only, which a parent's `FILE_LIST_DIRECTORY` grants), as
//!   `FILE_ID_INFO`; the owner, for information, through `acl::owner` (also opened as itself).
//! - A pass's folder is reached from `<data_dir>\sandbox` (made or secured just before by
//!   `write::make_sandbox_dir`, opened here by its path as itself and refused if a reparse
//!   point): each next part is made or opened relative to its parent's handle (`NtCreateFile`
//!   with `RootDirectory`, `FILE_OPEN_IF` or `FILE_OPEN`, `FILE_DIRECTORY_FILE`,
//!   `FILE_OPEN_REPARSE_POINT` and `OBJ_DONT_REPARSE`), and refused if a reparse point.
//! - An item is opened as itself with `DELETE | FILE_READ_ATTRIBUTES` alone, relative to its
//!   folder's handle (that folder opened by its path as itself, refused if a reparse point or
//!   if its final path isn't the planned one), its own final path checked against the planned
//!   one, and renamed on that handle (`SetFileInformationByHandle(FileRenameInfo)`) into the
//!   pass folder's handle (`RootDirectory`), `ReplaceIfExists` false. A restore opens the item
//!   relative to its pass's folder and renames it into its old folder, opened and checked the
//!   same way.
//!
//! A rename across volumes fails with `ERROR_NOT_SAME_DEVICE`: [`SetAsideFailure::OtherVolume`].
#![expect(
    unsafe_code,
    reason = "FFI to open the set-aside's folders and items by handle and rename them"
)]

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_FOR_BACKUP_INTENT, FILE_OPEN_IF,
    FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_DISPOSITION,
    NTCREATEFILE_CREATE_OPTIONS, NtCreateFile,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_NOT_SAME_DEVICE, HANDLE, NTSTATUS, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE,
    RtlNtStatusToDosError, UNICODE_STRING,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, DELETE, FILE_ACCESS_RIGHTS, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES, FILE_ID_INFO, FILE_NAME_NORMALIZED,
    FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_TRAVERSE, FileAttributeTagInfo, FileIdInfo, FileRenameInfo,
    GETFINALPATHNAMEBYHANDLE_FLAGS, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
    OPEN_EXISTING, SYNCHRONIZE, SetFileInformationByHandle, VOLUME_NAME_DOS,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::core::{PCWSTR, PWSTR};

use super::SetAsideFailure;
use super::files::{FileId, io_failure};
use super::run::Volume;
use crate::windows::folder::verbatim_wide;

/// `ERROR_REPARSE_POINT_ENCOUNTERED`: an open told not to follow a reparse point met one.
const REPARSE_ENCOUNTERED: i32 = 4395;

/// What a folder on the way to a pass's folder is opened with: pass through it, add a file or
/// folder to it (a part made in it, an item renamed into it), read its attributes.
const FOLDER: u32 = FILE_TRAVERSE.0
    | FILE_ADD_FILE.0
    | FILE_ADD_SUBDIRECTORY.0
    | FILE_READ_ATTRIBUTES.0
    | SYNCHRONIZE.0;

/// What an item to move is opened with: no `SYNCHRONIZE`, which `CreateFileW` would add, so an
/// item whose own DACL shuts the lawyer out still opens through its folder's `FILE_DELETE_CHILD`
/// and `FILE_LIST_DIRECTORY`, as `DeleteFileW`'s open does.
const MOVE: u32 = DELETE.0 | FILE_READ_ATTRIBUTES.0;

/// What an item's folder in the matter is opened with, to open the item relative to it.
const PARENT: u32 = FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;

/// The set-aside's [`Volume`] on NTFS.
pub(super) struct Ntfs;

impl Volume for Ntfs {
    fn file_id(&self, path: &Path) -> io::Result<Option<FileId>> {
        let handle = match open_path(path, FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0) {
            Ok(handle) => handle,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut info = FILE_ID_INFO::default();
        // SAFETY: the handle is open and `info` is valid and writable for its size.
        unsafe {
            GetFileInformationByHandleEx(
                handle.0,
                FileIdInfo,
                (&raw mut info).cast(),
                u32::try_from(size_of::<FILE_ID_INFO>()).unwrap_or(0),
            )
        }
        .map_err(|e| os_error(&e))?;
        Ok(Some(FileId {
            volume: info.VolumeSerialNumber,
            id: info.FileId.Identifier,
        }))
    }

    fn owner(&self, path: &Path) -> io::Result<String> {
        crate::windows::acl::owner(path).map(|sid| sid.to_string())
    }

    fn move_into(
        &self,
        from: &Path,
        sandbox: &Path,
        parts: &[String],
        name: &str,
    ) -> Result<(), SetAsideFailure> {
        let folder = folder_chain(sandbox, parts, FILE_OPEN_IF)?;
        let parent_path = from
            .parent()
            .ok_or(SetAsideFailure::Io(io::ErrorKind::InvalidInput))?;
        let leaf = from
            .file_name()
            .ok_or(SetAsideFailure::Io(io::ErrorKind::InvalidInput))?;
        let parent = open_folder(parent_path, PARENT)?;
        let leaf: Vec<u16> = leaf.encode_wide().collect();
        let item = open_at(
            &parent,
            &leaf,
            MOVE,
            FILE_OPEN,
            NTCREATEFILE_CREATE_OPTIONS(0),
        )
        .map_err(|e| gone_or(&e))?;
        if !same_path(&item, from).map_err(|e| io_failure(&e))? {
            return Err(SetAsideFailure::Changed);
        }
        rename(&item, &folder, &wide(name)).map_err(|e| failure(&e))
    }

    fn move_back(
        &self,
        sandbox: &Path,
        parts: &[String],
        name: &str,
        to: &Path,
    ) -> Result<(), SetAsideFailure> {
        let folder = folder_chain(sandbox, parts, FILE_OPEN).map_err(|failure| {
            if failure == SetAsideFailure::Io(io::ErrorKind::NotFound) {
                SetAsideFailure::Gone
            } else {
                failure
            }
        })?;
        let options = NTCREATEFILE_CREATE_OPTIONS(0);
        let item =
            open_at(&folder, &wide(name), MOVE, FILE_OPEN, options).map_err(|e| gone_or(&e))?;
        let parent_path = to.parent().ok_or(SetAsideFailure::Record)?;
        let leaf = to.file_name().ok_or(SetAsideFailure::Record)?;
        let parent = open_folder(parent_path, FOLDER)?;
        let leaf: Vec<u16> = leaf.encode_wide().collect();
        rename(&item, &parent, &leaf).map_err(|e| failure(&e))
    }
}

/// Opens the folder `path` in the matter by its path as itself, for `access`; refused when it is
/// a reparse point or its final path isn't `path`.
fn open_folder(path: &Path, access: u32) -> Result<Handle, SetAsideFailure> {
    let folder = open_path(path, access).map_err(|e| failure(&e))?;
    if reparse(&folder).map_err(|e| io_failure(&e))?
        || !same_path(&folder, path).map_err(|e| io_failure(&e))?
    {
        return Err(SetAsideFailure::Changed);
    }
    Ok(folder)
}

/// Opens `sandbox` by its path as itself, then each of `parts` relative to the one before with
/// `disposition`; refuses any that is a reparse point. Returns the last.
fn folder_chain(
    sandbox: &Path,
    parts: &[String],
    disposition: NTCREATEFILE_CREATE_DISPOSITION,
) -> Result<Handle, SetAsideFailure> {
    let mut folder = open_path(sandbox, FOLDER).map_err(|e| failure(&e))?;
    if reparse(&folder).map_err(|e| io_failure(&e))? {
        return Err(SetAsideFailure::Changed);
    }
    for part in parts {
        let next = open_at(
            &folder,
            &wide(part),
            FOLDER,
            disposition,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
        )
        .map_err(|e| failure(&e))?;
        if reparse(&next).map_err(|e| io_failure(&e))? {
            tracing::warn!("a set-aside folder is a link; nothing is moved through it");
            return Err(SetAsideFailure::Changed);
        }
        folder = next;
    }
    Ok(folder)
}

/// Renames the item `item` (opened with `DELETE`) to `name` in the folder `folder`, never
/// replacing what is there.
fn rename(item: &Handle, folder: &Handle, name: &[u16]) -> io::Result<()> {
    let header = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let name_bytes = name.len() * 2;
    let size = (header + name_bytes + 2).max(size_of::<FILE_RENAME_INFO>());
    let length = u32::try_from(name_bytes).map_err(io::Error::other)?;
    let size_u32 = u32::try_from(size).map_err(io::Error::other)?;
    // u64 words, so the structure is aligned; zeroed, so `ReplaceIfExists` starts false and
    // the name ends in a NUL.
    let mut buffer = vec![0_u64; size.div_ceil(8)];
    let base = buffer.as_mut_ptr();
    let info = base.cast::<FILE_RENAME_INFO>();
    // SAFETY: `buffer` is at least `size_of::<FILE_RENAME_INFO>()` bytes, aligned for it and
    // zeroed; the name's `name_bytes` bytes fit from `header` on, as `size` counts them; the
    // pointers stay inside `buffer`, which outlives the call; both handles are open.
    unsafe {
        (&raw mut (*info).Anonymous.ReplaceIfExists).write(false);
        (&raw mut (*info).RootDirectory).write(folder.0);
        (&raw mut (*info).FileNameLength).write(length);
        let start = base.cast::<u8>().add(header);
        std::ptr::copy_nonoverlapping(name.as_ptr().cast::<u8>(), start, name_bytes);
        SetFileInformationByHandle(item.0, FileRenameInfo, base.cast_const().cast(), size_u32)
    }
    .map_err(|e| os_error(&e))
}

/// Opens `path` itself, a folder too, never what a link at it names, for `access`.
fn open_path(path: &Path, access: u32) -> io::Result<Handle> {
    let wide = verbatim_wide(path)?;
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes or
    // template are passed, and the handle is owned by `Handle` from here.
    unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            share,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map(Handle)
    .map_err(|e| os_error(&e))
}

/// Opens or makes `name` (one path part, no NUL) in the folder `folder` relative to its
/// handle, opening a reparse point itself, and failing on any reparse met on the way
/// (`OBJ_DONT_REPARSE`, after Codex's `no_reparse_dir.rs`, as `acl/object.rs` does). `options`
/// says whether it must be a folder and whether its I/O is synchronous (which needs
/// `SYNCHRONIZE` in `access`).
fn open_at(
    folder: &Handle,
    name: &[u16],
    access: u32,
    disposition: NTCREATEFILE_CREATE_DISPOSITION,
    options: NTCREATEFILE_CREATE_OPTIONS,
) -> io::Result<Handle> {
    let bytes = u16::try_from(name.len() * 2)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let unicode = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(name.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
        RootDirectory: folder.0,
        ObjectName: &raw const unicode,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        ..OBJECT_ATTRIBUTES::default()
    };
    let mut handle = HANDLE::default();
    let mut status = IO_STATUS_BLOCK::default();
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let options = options | FILE_OPEN_REPARSE_POINT | FILE_OPEN_FOR_BACKUP_INTENT;
    // SAFETY: `unicode` points into `name`, which outlives the call and is never written;
    // `attributes` and the out-pointers are live; the folder's handle is open; the handle
    // made is owned by `Handle` from here.
    let done = unsafe {
        NtCreateFile(
            &raw mut handle,
            FILE_ACCESS_RIGHTS(access),
            &raw const attributes,
            &raw mut status,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            share,
            disposition,
            options,
            None,
            0,
        )
    };
    if done.is_err() {
        return Err(nt_error(done));
    }
    Ok(Handle(handle))
}

/// Whether the handle's file or folder is a reparse point.
fn reparse(handle: &Handle) -> io::Result<bool> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle is open and `tag` is valid and writable for its size.
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileAttributeTagInfo,
            (&raw mut tag).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0),
        )
    }
    .map_err(|e| os_error(&e))?;
    Ok(tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0)
}

/// Whether the handle's final path (normalized, with its drive letter) is `planned`, compared
/// part by part without ASCII case, `\\?\` set aside.
fn same_path(handle: &Handle, planned: &Path) -> io::Result<bool> {
    let found = PathBuf::from(OsString::from_wide(&final_path(handle)?));
    let found = dunce::simplified(&found);
    let planned = std::path::absolute(planned)?;
    let planned = dunce::simplified(&planned);
    let (mut a, mut b) = (found.components(), planned.components());
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ok(true),
            (Some(x), Some(y)) if x.as_os_str().eq_ignore_ascii_case(y.as_os_str()) => {}
            _ => return Ok(false),
        }
    }
}

/// The handle's final path, normalized, with its drive letter (`\\?\C:\...`).
fn final_path(handle: &Handle) -> io::Result<Vec<u16>> {
    let flags = GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0);
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: the handle is open and `buffer` is a writable slice of its length.
        let length = unsafe { GetFinalPathNameByHandleW(handle.0, &mut buffer, flags) };
        match usize::try_from(length).map_err(io::Error::other)? {
            0 => return Err(io::Error::last_os_error()),
            n if n < buffer.len() => {
                buffer.truncate(n);
                return Ok(buffer);
            }
            n => buffer.resize(n + 1, 0),
        }
    }
}

/// `text` as UTF-16, without a NUL.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// An item's open failing: [`SetAsideFailure::Gone`] when it isn't there, else as [`failure`].
fn gone_or(error: &io::Error) -> SetAsideFailure {
    if error.kind() == io::ErrorKind::NotFound {
        SetAsideFailure::Gone
    } else {
        failure(error)
    }
}

/// A failed call as a set-aside failure: another volume, a reparse point met, a taken name, or
/// its kind.
fn failure(error: &io::Error) -> SetAsideFailure {
    let code = error.raw_os_error();
    if code.is_some() && code == i32::try_from(ERROR_NOT_SAME_DEVICE.0).ok() {
        SetAsideFailure::OtherVolume
    } else if code == Some(REPARSE_ENCOUNTERED) {
        SetAsideFailure::Changed
    } else if error.kind() == io::ErrorKind::AlreadyExists {
        SetAsideFailure::NameTaken
    } else {
        io_failure(error)
    }
}

/// A handle, closed on drop.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW or NtCreateFile and is closed once, here.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            tracing::debug!(code = error.code().0, "CloseHandle failed");
        }
    }
}

/// The Win32 error `error` carries, as an `io::Error` whose kind follows its code.
fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xFFFF)
}

/// The NT status `status`, as the Win32 error it maps to.
fn nt_error(status: NTSTATUS) -> io::Error {
    // SAFETY: RtlNtStatusToDosError only maps a number.
    let code = unsafe { RtlNtStatusToDosError(status) };
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}
