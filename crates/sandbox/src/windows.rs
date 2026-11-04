//! Windows: whether an agent sandbox that grants by ACL can prepare a folder. [`folder_support`]
//! asks the folder and its volume a few questions, in order, and stops at the first answer. It
//! writes nothing and takes no lock, and it answers for the calling process's own token, which
//! the agent inherits.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 file, volume and security calls"
)]

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, HANDLE};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetDriveTypeW,
    GetFileInformationByHandle, GetFinalPathNameByHandleW, GetVolumeInformationByHandleW,
    GetVolumePathNameW, OPEN_EXISTING, READ_CONTROL, VOLUME_NAME_DOS, WRITE_DAC,
};
use windows::Win32::System::SystemServices::FILE_PERSISTENT_ACLS;
use windows::Win32::System::WindowsProgramming::DRIVE_REMOTE;
use windows::core::PCWSTR;

/// Whether an agent sandbox that grants by ACL can prepare a folder on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderSupport {
    /// A local volume with permissions, and the caller can change the folder's permissions.
    Ok,
    /// The caller can't change the folder's permissions (an open for `WRITE_DAC` is denied).
    CantChangePermissions,
    /// The folder is on a network drive: a UNC path, a mapped drive, or a link that resolves to
    /// one.
    Network,
    /// The folder's volume keeps no permissions (FAT, exFAT).
    NoPermissions,
}

/// Asks whether the folder at `path` can take the agent's permission entries, in four steps (the
/// first that answers wins):
///
/// 1. Open the folder. A missing path is `NotFound`; a file is `InvalidInput`.
/// 2. `Network` when the folder's final path is a UNC path, or its volume's drive type is
///    `DRIVE_REMOTE`.
/// 3. `NoPermissions` when the volume lacks `FILE_PERSISTENT_ACLS`.
/// 4. `CantChangePermissions` when opening the folder for `READ_CONTROL | WRITE_DAC` is denied
///    (`ERROR_ACCESS_DENIED`); any other failure is an error. Otherwise `Ok`.
///
/// Every handle is closed on every path. A path over `MAX_PATH` goes to the Win32 calls in the
/// `\\?\` form; no test makes one yet. The answer is for this process's token, which the agent
/// inherits. A token with `SeBackupPrivilege` or `SeRestorePrivilege` enabled would pass step 4
/// whatever the folder's DACL says (`FILE_FLAG_BACKUP_SEMANTICS`); Clauseo never enables them, and
/// an administrator's token has them disabled.
pub fn folder_support(path: &Path) -> io::Result<FolderSupport> {
    let wide = verbatim_wide(path)?;
    let folder = open(&wide, FILE_READ_ATTRIBUTES.0).map_err(|error| os_error(&error))?;
    if !is_directory(&folder)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a folder", path.display()),
        ));
    }
    let final_path = final_path(&folder)?;
    if final_path.starts_with(&wide_text(r"\\?\UNC\")) || drive_type(&wide)? == DRIVE_REMOTE {
        return Ok(FolderSupport::Network);
    }
    if volume_flags(&folder)? & FILE_PERSISTENT_ACLS == 0 {
        return Ok(FolderSupport::NoPermissions);
    }
    match open(&wide, READ_CONTROL.0 | WRITE_DAC.0) {
        Ok(handle) => {
            drop(handle);
            Ok(FolderSupport::Ok)
        }
        Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => {
            Ok(FolderSupport::CantChangePermissions)
        }
        Err(error) => Err(os_error(&error)),
    }
}

/// A file or folder handle, closed on drop.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful CreateFileW and is closed only here, once.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            tracing::debug!(code = error.code().0, "CloseHandle failed");
        }
    }
}

/// Opens `wide` (NUL-terminated, verbatim) for `access`, as a folder: `OPEN_EXISTING` with
/// `FILE_FLAG_BACKUP_SEMANTICS`, and all three share modes so the check never blocks a tool that
/// has the folder open.
fn open(wide: &[u16], access: u32) -> windows::core::Result<Handle> {
    let share = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    // SAFETY: `wide` is NUL-terminated UTF-16 that outlives the call; no security attributes or
    // template handle are passed, and the returned handle is owned by `Handle` from here.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            share,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }?;
    Ok(Handle(handle))
}

/// Whether the open handle is a folder.
fn is_directory(folder: &Handle) -> io::Result<bool> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open and `info` is a valid, writable struct for the call.
    unsafe { GetFileInformationByHandle(folder.0, &raw mut info) }
        .map_err(|error| os_error(&error))?;
    Ok(info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0)
}

/// The folder's final path in the `\\?\` form (`\\?\UNC\server\share\...` for a network path).
fn final_path(folder: &Handle) -> io::Result<Vec<u16>> {
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: the handle is open and `buffer` is a writable slice of the length given.
        let length = unsafe { GetFinalPathNameByHandleW(folder.0, &mut buffer, VOLUME_NAME_DOS) };
        let length = usize::try_from(length).map_err(io::Error::other)?;
        match length {
            0 => return Err(os_error(&windows::core::Error::from_thread())),
            // A length within the buffer is the path without its NUL; a longer one is the size
            // the path needs, NUL included, so the next call fits.
            n if n < buffer.len() => {
                buffer.truncate(n);
                return Ok(buffer);
            }
            n => buffer.resize(n + 1, 0),
        }
    }
}

/// The drive type of the folder's volume root, from `GetVolumePathNameW` on `wide`.
fn drive_type(wide: &[u16]) -> io::Result<u32> {
    // The plain form (`C:\...`, `\\server\share\...`) while the path fits in `MAX_PATH`, which
    // every volume call takes; the `\\?\` form only beyond that. `wide` ends in its NUL.
    let input = if wide.len() <= MAX_PATH {
        plain_text(wide)
    } else {
        wide.to_vec()
    };
    // The root is a prefix of the path, so a buffer of the input's length fits it with its NUL.
    let mut root = vec![0u16; input.len()];
    // SAFETY: `input` is NUL-terminated UTF-16 that outlives the call, and `root` is a writable
    // buffer of the length given.
    unsafe { GetVolumePathNameW(PCWSTR(input.as_ptr()), &mut root) }
        .map_err(|error| os_error(&error))?;
    let text = root.split(|&unit| unit == 0).next().unwrap_or_default();
    let mut plain = plain_text(text);
    plain.push(0);
    // SAFETY: `plain` is NUL-terminated UTF-16 that outlives the call.
    Ok(unsafe { GetDriveTypeW(PCWSTR(plain.as_ptr())) })
}

/// The volume's file-system flags.
fn volume_flags(folder: &Handle) -> io::Result<u32> {
    let mut flags = 0_u32;
    // SAFETY: the handle is open; `flags` is a valid, writable u32, and the buffers are left out.
    unsafe {
        GetVolumeInformationByHandleW(folder.0, None, None, None, Some(&raw mut flags), None)
    }
    .map_err(|error| os_error(&error))?;
    Ok(flags)
}

/// `path` as an absolute, NUL-terminated UTF-16 string in the `\\?\` form, which lifts the
/// `MAX_PATH` limit. A UNC path becomes `\\?\UNC\...`.
fn verbatim_wide(path: &Path) -> io::Result<Vec<u16>> {
    let absolute = std::path::absolute(path)?;
    let text: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let verbatim = wide_text(r"\\?\");
    let mut wide = if text.starts_with(&verbatim) {
        text
    } else if let Some(rest) = text.strip_prefix(wide_text(r"\\").as_slice()) {
        let mut wide = wide_text(r"\\?\UNC\");
        wide.extend_from_slice(rest);
        wide
    } else {
        let mut wide = verbatim;
        wide.extend_from_slice(&text);
        wide
    };
    wide.push(0);
    Ok(wide)
}

/// `text` as UTF-16, without a NUL.
fn wide_text(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// `text` as a drive or UNC path, the form `GetDriveTypeW` takes: the `\\?\` prefix goes, and
/// `\\?\UNC\` becomes `\\`.
fn plain_text(text: &[u16]) -> Vec<u16> {
    if let Some(rest) = text.strip_prefix(wide_text(r"\\?\UNC\").as_slice()) {
        let mut plain = wide_text(r"\\");
        plain.extend_from_slice(rest);
        plain
    } else if let Some(rest) = text.strip_prefix(wide_text(r"\\?\").as_slice()) {
        rest.to_vec()
    } else {
        text.to_vec()
    }
}

/// `MAX_PATH`, counted with the NUL.
const MAX_PATH: usize = 260;

/// The Win32 error that `error` carries, as an `io::Error`. The calls here report
/// `HRESULT_FROM_WIN32(code)`, which keeps the code in its low 16 bits.
fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xFFFF)
}

#[cfg(test)]
mod tests;
