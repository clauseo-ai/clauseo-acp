//! Windows: the download mover's folder, opened once as a handle, and every change in it made
//! relative to that handle (`write::Folder`, `write/copy.rs`), so a junction or symlink swapped
//! in for the folder after it was opened is never followed:
//! - the folder: `CreateFileW` with `FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT`,
//!   then a link at its name is refused (std's `File::metadata` of the handle);
//! - a name in it: `NtCreateFile` with `RootDirectory` the folder's handle, `FILE_CREATE` (never
//!   replaces) and `FILE_OPEN_REPARSE_POINT` (never follows);
//! - a move or rename into it: `NtSetInformationFile(FileRenameInformation)` on the file's own
//!   handle with `RootDirectory` the folder's handle and `ReplaceIfExists` false (std has no
//!   rename that never replaces, and kernel32's `SetFileInformationByHandle(FileRenameInfo)`
//!   refuses a `RootDirectory`);
//! - the copy's mark: its `:Zone.Identifier` stream opened with `RootDirectory` the copy's own
//!   handle;
//! - a delete: `FileDispositionInfo` on the file's own handle;
//! - a listing: `GetFileInformationByHandleEx(FileFullDirectoryInfo)` on the folder's handle;
//! - a copy's temporary file is made hidden (`FILE_ATTRIBUTE_HIDDEN`), and shown again
//!   (`FileBasicInfo`) just before it gets its name.
//!
//! And the proxy's secret file, checked and deleted through the one handle it was opened with
//! (`write/secret.rs`): `GetFileInformationByHandle` and `delete_by_handle`.
#![expect(
    unsafe_code,
    reason = "NtCreateFile, NtSetInformationFile, SetFileInformationByHandle and \
              GetFileInformationByHandleEx, relative to handles; std has none of them; the secret \
              file's GetFileInformationByHandle"
)]

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::Path;

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
    FILE_RENAME_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT, FileRenameInformation,
    NTCREATEFILE_CREATE_DISPOSITION, NtCreateFile, NtSetInformationFile,
};
use windows::Win32::Foundation::{
    HANDLE, NTSTATUS, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, STATUS_OBJECT_NAME_NOT_FOUND,
    STATUS_OBJECT_PATH_NOT_FOUND, UNICODE_STRING, WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ACCESS_RIGHTS, FILE_ATTRIBUTE_HIDDEN,
    FILE_ATTRIBUTE_NORMAL, FILE_BASIC_INFO, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES, FILE_FULL_DIR_INFO, FILE_GENERIC_READ,
    FILE_GENERIC_WRITE, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
    FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FileBasicInfo,
    FileDispositionInfo, FileFullDirectoryInfo, FileFullDirectoryRestartInfo,
    GetFileInformationByHandle, GetFileInformationByHandleEx, SYNCHRONIZE,
    SetFileInformationByHandle,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::core::PWSTR;

/// A struct's size as the `u32` Win32 takes (every one here is a few bytes).
fn size_u32(size: usize) -> u32 {
    u32::try_from(size).unwrap_or(u32::MAX)
}

/// Every sharing mode: the guard's handles never stop Chrome, a scanner or the lawyer.
const SHARE_ALL: FILE_SHARE_MODE =
    FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0);

/// Opens `folder` itself (`FILE_FLAG_OPEN_REPARSE_POINT`: a junction or symlink at its name is
/// opened, not followed), sharing everything, then refuses it unless it is a real folder. Only
/// the last part of the path is held to that, as on Unix.
pub(super) fn open_folder(folder: &Path) -> io::Result<OwnedHandle> {
    let opened = std::fs::OpenOptions::new()
        .access_mode((FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE).0)
        .share_mode(SHARE_ALL.0)
        .custom_flags((FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).0)
        .open(folder)
        .map_err(|error| failed_at("CreateFileW(the folder)", error))?;
    let kind = opened.metadata()?.file_type();
    if kind.is_symlink() || !kind.is_dir() {
        return Err(io::Error::other("the folder is no longer a real folder"));
    }
    Ok(OwnedHandle::from(opened))
}

/// Whether `name` exists in the folder `dir` holds, as anything, a link included: an open of
/// its attributes relative to the handle. Only "not found" reads as absent; any other failure
/// (a sharing violation, a denial) reads as present, so the mover takes another name.
pub(super) fn exists_in(dir: &OwnedHandle, name: &OsStr) -> bool {
    match nt_open(
        dir,
        name,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_OPEN_REPARSE_POINT.0,
        FILE_ATTRIBUTE_NORMAL,
    ) {
        Ok(_found) => true,
        Err(status) => {
            status != STATUS_OBJECT_NAME_NOT_FOUND && status != STATUS_OBJECT_PATH_NOT_FOUND
        }
    }
}

/// Makes the file `name` in the folder `dir` holds, new (`FILE_CREATE`), open to read, write
/// and delete (the copy deletes its temporary file through this handle).
pub(super) fn create_new_in(dir: &OwnedHandle, name: &OsStr) -> io::Result<File> {
    create_in(dir, name, FILE_ATTRIBUTE_NORMAL)
}

/// [`create_new_in`], the file hidden (`FILE_ATTRIBUTE_HIDDEN`): a copy's temporary file, which
/// a crash would leave behind. [`unhide`] shows it again before it gets its name.
pub(super) fn create_hidden_in(dir: &OwnedHandle, name: &OsStr) -> io::Result<File> {
    create_in(dir, name, FILE_ATTRIBUTE_HIDDEN)
}

fn create_in(
    dir: &OwnedHandle,
    name: &OsStr,
    attributes: FILE_FLAGS_AND_ATTRIBUTES,
) -> io::Result<File> {
    nt_open(
        dir,
        name,
        FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE,
        FILE_CREATE,
        FILE_NON_DIRECTORY_FILE.0 | FILE_OPEN_REPARSE_POINT.0,
        attributes,
    )
    .map(File::from)
    .map_err(|status| nt_error("NtCreateFile(a new file, relative to the folder)", status))
}

/// Clears `FILE_ATTRIBUTE_HIDDEN` on the open `file` (`FileBasicInfo` through its handle; the
/// times left as they are, which zero means).
pub(super) fn unhide(file: &File) -> io::Result<()> {
    let attributes = file.metadata()?.file_attributes() & !FILE_ATTRIBUTE_HIDDEN.0;
    let info = FILE_BASIC_INFO {
        // Zero would leave the attributes as they are.
        FileAttributes: if attributes == 0 {
            FILE_ATTRIBUTE_NORMAL.0
        } else {
            attributes
        },
        ..FILE_BASIC_INFO::default()
    };
    // SAFETY: `info` is a valid `FILE_BASIC_INFO` that outlives the call, and its size is the
    // one passed; `file`'s handle is open for the call.
    unsafe {
        SetFileInformationByHandle(
            handle(file.as_raw_handle()),
            FileBasicInfo,
            std::ptr::from_ref(&info).cast(),
            size_u32(size_of::<FILE_BASIC_INFO>()),
        )
    }
    .map_err(|error| {
        failed_at(
            "SetFileInformationByHandle(FileBasicInfo, unhide)",
            io_error(&error),
        )
    })
}

/// Deletes `name` from the folder `dir` holds when it is a plain file: opened relative to the
/// handle with `FILE_OPEN_REPARSE_POINT` (a link is opened itself, never followed) and
/// `FILE_NON_DIRECTORY_FILE`, refused unless a plain file, then deleted through that handle.
pub(super) fn remove_plain_in(dir: &OwnedHandle, name: &OsStr) -> io::Result<()> {
    let file = nt_open(
        dir,
        name,
        DELETE | FILE_READ_ATTRIBUTES,
        FILE_OPEN,
        FILE_NON_DIRECTORY_FILE.0 | FILE_OPEN_REPARSE_POINT.0,
        FILE_ATTRIBUTE_NORMAL,
    )
    .map(File::from)
    .map_err(|status| {
        nt_error(
            "NtCreateFile(a file to remove, relative to the folder)",
            status,
        )
    })?;
    let kind = file.metadata()?.file_type();
    if kind.is_symlink() || !kind.is_file() {
        return Err(io::Error::other("not a plain file"));
    }
    delete_by_handle(&file)
}

/// How many bytes one listing call reads.
const LISTING_BYTES: usize = 64 * 1024;

/// The names in the folder `dir` holds, `.` and `..` left out:
/// `GetFileInformationByHandleEx(FileFullDirectoryInfo)` on its handle, never its path. What
/// was read before a failure.
pub(super) fn names_in(dir: &OwnedHandle) -> Vec<OsString> {
    let mut names = Vec::new();
    let mut buffer = vec![0_u64; LISTING_BYTES / size_of::<u64>()];
    let mut class = FileFullDirectoryRestartInfo;
    // SAFETY: `buffer` is writable for the size passed and `dir`'s handle is open for the call.
    while unsafe {
        GetFileInformationByHandleEx(
            handle(dir.as_raw_handle()),
            class,
            buffer.as_mut_ptr().cast(),
            size_u32(LISTING_BYTES),
        )
    }
    .is_ok()
    {
        class = FileFullDirectoryInfo;
        // SAFETY: `buffer` holds `LISTING_BYTES` initialised bytes, read as bytes (`u8` is
        // aligned anywhere) while the slice lives, and nothing writes it meanwhile.
        let bytes =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), LISTING_BYTES) };
        listed(bytes, &mut names);
    }
    names
}

/// The names in `bytes`, a chain of `FILE_FULL_DIR_INFO` entries, read field by field with
/// every offset checked against the buffer's end; pushed to `names` but `.` and `..`.
fn listed(bytes: &[u8], names: &mut Vec<OsString>) {
    let header = std::mem::offset_of!(FILE_FULL_DIR_INFO, FileName);
    let length_at = std::mem::offset_of!(FILE_FULL_DIR_INFO, FileNameLength);
    let next_at = std::mem::offset_of!(FILE_FULL_DIR_INFO, NextEntryOffset);
    let u32_at = |at: usize| -> Option<usize> {
        let field: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
        usize::try_from(u32::from_ne_bytes(field)).ok()
    };
    let mut offset = 0_usize;
    loop {
        let (Some(next), Some(length)) = (u32_at(offset + next_at), u32_at(offset + length_at))
        else {
            return;
        };
        let Some(name) = bytes.get(offset + header..offset + header + length) else {
            return;
        };
        let wide: Vec<u16> = name
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_ne_bytes(*pair))
            .collect();
        let name = OsString::from_wide(&wide);
        if name != "." && name != ".." {
            names.push(name);
        }
        if next == 0 {
            return;
        }
        offset += next;
    }
}

/// Opens the file `from` (in staging, the guard's own folder) to copy it or move it: read,
/// delete and its attributes, sharing everything, a reparse point at its name opened itself;
/// anything but a plain file is refused. A file another process holds without delete sharing
/// fails the open with a sharing violation, retried on the next sweep.
pub(super) fn open_source(from: &Path) -> io::Result<File> {
    let opened = std::fs::OpenOptions::new()
        .access_mode((FILE_GENERIC_READ | DELETE).0)
        .share_mode(SHARE_ALL.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(from)
        .map_err(|error| failed_at("CreateFileW(the staging file)", error))?;
    let kind = opened.metadata()?.file_type();
    if kind.is_symlink() || !kind.is_file() {
        return Err(io::Error::other("not a plain file"));
    }
    Ok(opened)
}

/// Moves the file `from` into the folder `dir` holds as `name`, within one volume, never
/// replacing a file: the file opened by its path in staging ([`open_source`]), then renamed
/// through its handle relative to the folder's ([`rename_into`]). Another volume fails with
/// `CrossesDevices` (`ERROR_NOT_SAME_DEVICE`).
pub(super) fn move_new_into(from: &Path, dir: &OwnedHandle, name: &OsStr) -> io::Result<()> {
    rename_into(&open_source(from)?, dir, name)
}

/// Renames the open `file` to `name` in the folder `dir` holds: ntdll's
/// `NtSetInformationFile(FileRenameInformation)` with `ReplaceIfExists` false, `RootDirectory`
/// the folder's handle and `FileName` the plain name relative to it. Not kernel32's
/// `SetFileInformationByHandle(FileRenameInfo)`, which refuses a `RootDirectory` with
/// `ERROR_INVALID_PARAMETER`. `AlreadyExists` for a taken name (`STATUS_OBJECT_NAME_COLLISION`),
/// `CrossesDevices` for another volume (`STATUS_NOT_SAME_DEVICE`).
pub(super) fn rename_into(file: &File, dir: &OwnedHandle, name: &OsStr) -> io::Result<()> {
    let wide: Vec<u16> = name.encode_wide().collect();
    let name_bytes = wide.len() * size_of::<u16>();
    let offset = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
    let size = (offset + name_bytes).max(size_of::<FILE_RENAME_INFORMATION>());
    // `u64`s, so the buffer is aligned for `FILE_RENAME_INFORMATION` (8 bytes at most).
    let mut buffer = vec![0_u64; size.div_ceil(size_of::<u64>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    let length = u32::try_from(name_bytes).map_err(io::Error::other)?;
    let size = u32::try_from(size).map_err(io::Error::other)?;
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: `buffer` is zeroed (`Flags` 0: no replace), aligned and at least `size` bytes,
    // which holds the header and the name after `FileName`'s offset; the writes stay inside it.
    // The call reads `size` bytes and both handles are open for its length (`file` and `dir`
    // are borrowed); `status_block` outlives it.
    let status = unsafe {
        (*info).RootDirectory = handle(dir.as_raw_handle());
        (*info).FileNameLength = length;
        let name_at = info.cast::<u8>().add(offset);
        std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), name_at, name_bytes);
        NtSetInformationFile(
            handle(file.as_raw_handle()),
            &raw mut status_block,
            info.cast(),
            size,
            FileRenameInformation,
        )
    };
    if status.is_ok() {
        Ok(())
    } else {
        Err(nt_error(
            "NtSetInformationFile(FileRenameInformation, into the folder)",
            status,
        ))
    }
}

/// `GetFileInformationByHandle` of `file`: its attributes (`dwFileAttributes`) and how many
/// names it has (`nNumberOfLinks`).
pub(super) fn attributes_and_links(file: &File) -> io::Result<(u32, u32)> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is `file`'s own, open for the whole call; `info` is a live, writable
    // BY_HANDLE_FILE_INFORMATION that the call fills and nothing else borrows.
    unsafe { GetFileInformationByHandle(handle(file.as_raw_handle()), &raw mut info) }
        .map_err(|error| io_error(&error))?;
    Ok((info.dwFileAttributes, info.nNumberOfLinks))
}

/// Deletes the open `file` through its handle (`FileDispositionInfo`), once every handle to it
/// is closed; the handle must have been opened with `DELETE`.
pub(super) fn delete_by_handle(file: &File) -> io::Result<()> {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `info` is a valid `FILE_DISPOSITION_INFO` that outlives the call, and its size is
    // the one passed; `file`'s handle is open for the call.
    unsafe {
        SetFileInformationByHandle(
            handle(file.as_raw_handle()),
            FileDispositionInfo,
            std::ptr::from_ref(&info).cast(),
            size_u32(size_of::<FILE_DISPOSITION_INFO>()),
        )
    }
    .map_err(|error| {
        failed_at(
            "SetFileInformationByHandle(FileDispositionInfo, delete)",
            io_error(&error),
        )
    })
}

/// The stream `stream` (`:Zone.Identifier`) of the open `file`, opened relative to the file's
/// own handle: made new for writing when `create`, else opened to read. A volume without
/// alternate data streams (FAT, exFAT, some shares) fails.
pub(crate) fn open_stream(file: &File, stream: &str, create: bool) -> io::Result<File> {
    let (access, disposition) = if create {
        (FILE_GENERIC_WRITE, FILE_CREATE)
    } else {
        (FILE_GENERIC_READ, FILE_OPEN)
    };
    let root = file.as_raw_handle();
    // SAFETY: `root` is `file`'s handle, open for the call; it is borrowed, not owned, so the
    // `ManuallyDrop` keeps it from being closed here.
    let root = std::mem::ManuallyDrop::new(unsafe { OwnedHandle::from_raw_handle(root) });
    nt_open(
        &root,
        OsStr::new(stream),
        access,
        disposition,
        FILE_NON_DIRECTORY_FILE.0,
        FILE_ATTRIBUTE_NORMAL,
    )
    .map(File::from)
    .map_err(|status| {
        nt_error(
            "NtCreateFile(the :Zone.Identifier stream, relative to the file)",
            status,
        )
    })
}

/// `NtCreateFile` of `name` relative to `root`, case-insensitive as Win32 names are, for
/// synchronous I/O (std's `File` needs it), sharing everything, a new file made with
/// `new_attributes`; the status on failure.
fn nt_open(
    root: &OwnedHandle,
    name: &OsStr,
    access: FILE_ACCESS_RIGHTS,
    disposition: NTCREATEFILE_CREATE_DISPOSITION,
    options: u32,
    new_attributes: FILE_FLAGS_AND_ATTRIBUTES,
) -> Result<OwnedHandle, NTSTATUS> {
    let mut wide: Vec<u16> = name.encode_wide().collect();
    let bytes = u16::try_from(wide.len() * size_of::<u16>())
        .map_err(|_too_long| STATUS_OBJECT_NAME_NOT_FOUND)?;
    let unicode = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(wide.as_mut_ptr()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_u32(size_of::<OBJECT_ATTRIBUTES>()),
        RootDirectory: handle(root.as_raw_handle()),
        ObjectName: &raw const unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        ..OBJECT_ATTRIBUTES::default()
    };
    let mut opened = HANDLE::default();
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: every pointer is to a live local that outlives the call (`unicode` points into
    // `wide`); `root` is open for the call. On success the call gives `opened`, a handle this
    // function then owns.
    let status = unsafe {
        NtCreateFile(
            &raw mut opened,
            access | SYNCHRONIZE,
            &raw const attributes,
            &raw mut status_block,
            None,
            new_attributes,
            SHARE_ALL,
            disposition,
            windows::Wdk::Storage::FileSystem::NTCREATEFILE_CREATE_OPTIONS(
                options | FILE_SYNCHRONOUS_IO_NONALERT.0,
            ),
            None,
            0,
        )
    };
    if status.is_ok() {
        // SAFETY: the call succeeded, so `opened` is a new handle nothing else owns.
        Ok(unsafe { OwnedHandle::from_raw_handle(opened.0) })
    } else {
        Err(status)
    }
}

/// A raw handle as the `windows` crate's `HANDLE`.
fn handle(raw: RawHandle) -> HANDLE {
    HANDLE(raw)
}

/// A failed call's error, naming the call and its step: its `kind()` is the OS error's, and
/// the OS error is its `source()` (`crate::write::os_code` reads the code through it).
#[derive(Debug)]
struct Failed {
    call: String,
    source: io::Error,
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.call, self.source)
    }
}

impl std::error::Error for Failed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// `error` from `call` (the API and the step), keeping its kind.
fn failed_at(call: impl Into<String>, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        Failed {
            call: call.into(),
            source: error,
        },
    )
}

/// `status` from `call`, as std's own OS error through its Win32 code, so `kind()` is right
/// (`STATUS_OBJECT_NAME_COLLISION` reads as `AlreadyExists`), named with the call and the
/// status in hex: `NtCreateFile(...): NTSTATUS 0xc000000d: The parameter is incorrect. (os
/// error 87)`.
fn nt_error(call: &str, status: NTSTATUS) -> io::Error {
    // SAFETY: a pure lookup of a status code.
    let code = unsafe { RtlNtStatusToDosError(status) };
    let os = match i32::try_from(code) {
        Ok(code) => io::Error::from_raw_os_error(code),
        Err(_) => io::Error::other("no Win32 error for it"),
    };
    failed_at(
        format!("{call}: NTSTATUS {:#010x}", status.0.cast_unsigned()),
        os,
    )
}

/// The Win32 error inside `error` as std's own OS error, so `kind()` is right:
/// `ERROR_ALREADY_EXISTS` (183) and `ERROR_FILE_EXISTS` (80) read as `AlreadyExists`,
/// `ERROR_NOT_SAME_DEVICE` (17) as `CrossesDevices`. `io::Error::from` would keep the HRESULT
/// (`0x800700B7`), whose kind std does not know.
fn io_error(error: &windows::core::Error) -> io::Error {
    match WIN32_ERROR::from_error(error).and_then(|code| i32::try_from(code.0).ok()) {
        Some(code) => io::Error::from_raw_os_error(code),
        None => io::Error::from(error.clone()),
    }
}

#[cfg(test)]
mod tests;
