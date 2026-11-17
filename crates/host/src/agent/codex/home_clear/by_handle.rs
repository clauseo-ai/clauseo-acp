//! Windows: an entry of Codex's home that its own permissions keep from the usual rename (a
//! deny of reading, say, which also refuses the `SYNCHRONIZE` every `CreateFileW` open asks
//! for) is set aside without changing any permission. It is opened by its one name relative to
//! the home's handle, as itself (`FILE_OPEN_REPARSE_POINT`: a link is moved, never followed),
//! for `DELETE` alone, which the home's delete-child right normally grants whatever the entry's
//! own permissions say, through `NtCreateFile`, which adds no `SYNCHRONIZE`. It is then renamed
//! on that handle to its aside name, again relative to the home's handle and never replacing
//! anything (`FileRenameInformation`), and a file is marked for deletion on the same handle
//! (`FileDispositionInformation`; a folder with anything in it stays aside). A second name of a
//! file elsewhere moves or goes as a name alone: that file, and its permissions, stay as they
//! are.
#![expect(
    unsafe_code,
    reason = "FFI to NtCreateFile and NtSetInformationFile, relative to the home's handle"
)]

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_DISPOSITION_INFORMATION, FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION,
    FileDispositionInformation, FileRenameInformation, NtCreateFile, NtSetInformationFile,
};
use windows::Win32::Foundation::{
    CloseHandle, HANDLE, NTSTATUS, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_MODE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, OPEN_EXISTING,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::core::{PCWSTR, PWSTR};

/// Every sharing, so an open elsewhere neither stops this one nor is stopped by it; one that
/// shares no delete still stops it (a sharing violation, which the caller waits out).
const SHARE_ALL: FILE_SHARE_MODE =
    FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0);

/// Renames `home`'s entry `name` to `aside` there (both one name), through a delete-only handle,
/// and deletes it there when it is a file; the deletion is best-effort.
pub(super) fn rename_aside(home: &Path, name: &str, aside: &str) -> io::Result<()> {
    let (name, aside) = (one_name(name)?, one_name(aside)?);
    let folder = Handle::folder(home)?;
    let entry = Handle::delete_only(&folder, &name)?;
    entry.rename(&folder, &aside)?;
    // A folder with anything in it can't be deleted so; it stays aside for the sweep.
    drop(entry.mark_deleted());
    Ok(())
}

/// Deletes `home`'s file `name` (one name) through a delete-only handle: a file set aside that
/// its own permissions keep from the usual removal.
pub(super) fn delete(home: &Path, name: &str) -> io::Result<()> {
    let name = one_name(name)?;
    let folder = Handle::folder(home)?;
    Handle::delete_only(&folder, &name)?.mark_deleted()
}

/// `name` as UTF-16 when it is one name: not empty, not `.` or `..`, no separator, no `:` (no
/// stream or drive).
fn one_name(name: &str) -> io::Result<Vec<u16>> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['\\', '/', ':']) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    Ok(name.encode_utf16().collect())
}

/// An NTSTATUS failure as the Win32 error it stands for.
fn nt_error(status: NTSTATUS) -> io::Error {
    // SAFETY: a plain conversion of a status code to a Win32 error code.
    let code = unsafe { RtlNtStatusToDosError(status) };
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}

/// A handle, closed on drop.
struct Handle(HANDLE);

impl Handle {
    /// The folder `path` itself, opened to name entries relative to it.
    fn folder(path: &Path) -> io::Result<Self> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes and no
        // template; the handle made is owned by `Handle` from here.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0,
                SHARE_ALL,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                None,
            )
        }
        .map_err(io::Error::from)?;
        Ok(Self(handle))
    }

    /// `name` in `folder`, relative to its handle, itself (a link is opened, not followed), for
    /// `DELETE` alone.
    fn delete_only(folder: &Self, name: &[u16]) -> io::Result<Self> {
        let bytes = u16::try_from(name.len().saturating_mul(2)).map_err(io::Error::other)?;
        let unicode = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: PWSTR(name.as_ptr().cast_mut()),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).map_err(io::Error::other)?,
            RootDirectory: folder.0,
            ObjectName: &raw const unicode,
            Attributes: OBJ_CASE_INSENSITIVE,
            ..OBJECT_ATTRIBUTES::default()
        };
        let mut handle = HANDLE::default();
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: `unicode` points into `name`, which outlives the call and is never written;
        // `attributes` and the out-pointers are live; the folder's handle is open; the handle
        // made is owned by `Handle` from here.
        let done = unsafe {
            NtCreateFile(
                &raw mut handle,
                DELETE,
                &raw const attributes,
                &raw mut status,
                None,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                SHARE_ALL,
                FILE_OPEN,
                FILE_OPEN_REPARSE_POINT,
                None,
                0,
            )
        };
        if done.is_err() {
            return Err(nt_error(done));
        }
        Ok(Self(handle))
    }

    /// Renames the entry to `to`, a name in `folder` (relative to its handle), never replacing
    /// what is there.
    fn rename(&self, folder: &Self, to: &[u16]) -> io::Result<()> {
        let offset = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
        let name_bytes = to.len().saturating_mul(2);
        let length = u32::try_from(name_bytes).map_err(io::Error::other)?;
        let total = offset
            .saturating_add(name_bytes)
            .max(size_of::<FILE_RENAME_INFORMATION>());
        let size = u32::try_from(total).map_err(io::Error::other)?;
        let mut buffer = vec![0_u64; total.div_ceil(size_of::<u64>())];
        let base = buffer.as_mut_ptr();
        // SAFETY: `buffer` is `total` bytes or more, zeroed (no replace) and aligned for a
        // FILE_RENAME_INFORMATION (u64 words); its fields are written through `base`, and the
        // name's `to.len()` units go at the name's offset, which ends inside `buffer`.
        unsafe {
            let info = base.cast::<FILE_RENAME_INFORMATION>();
            (*info).RootDirectory = folder.0;
            (*info).FileNameLength = length;
            std::ptr::copy_nonoverlapping(
                to.as_ptr(),
                base.byte_add(offset).cast::<u16>(),
                to.len(),
            );
        }
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: the handle is open with DELETE; `buffer` holds `size` bytes of a
        // FILE_RENAME_INFORMATION and outlives the call; `status` is a live out-pointer.
        let done = unsafe {
            NtSetInformationFile(
                self.0,
                &raw mut status,
                buffer.as_ptr().cast(),
                size,
                FileRenameInformation,
            )
        };
        if done.is_err() {
            return Err(nt_error(done));
        }
        Ok(())
    }

    /// Marks the entry deleted when this handle closes (a folder only when it is empty).
    fn mark_deleted(&self) -> io::Result<()> {
        let info = FILE_DISPOSITION_INFORMATION { DeleteFile: true };
        let size =
            u32::try_from(size_of::<FILE_DISPOSITION_INFORMATION>()).map_err(io::Error::other)?;
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: the handle is open with DELETE; `info` is a live FILE_DISPOSITION_INFORMATION
        // of `size` bytes; `status` is a live out-pointer.
        let done = unsafe {
            NtSetInformationFile(
                self.0,
                &raw mut status,
                (&raw const info).cast(),
                size,
                FileDispositionInformation,
            )
        };
        if done.is_err() {
            return Err(nt_error(done));
        }
        Ok(())
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW or NtCreateFile and is closed once, here.
        drop(unsafe { CloseHandle(self.0) });
    }
}
