//! The refresh's reads of a file by its path, for the files under Program Files the refresh owns
//! (the staged copy, the admin copy), and the handle checks `walk.rs` uses on the install
//! folder's files: opened with `FILE_FLAG_OPEN_REPARSE_POINT`, so a link at the name is opened as
//! itself and refused by its tag, never followed, and with `FILE_FLAG_BACKUP_SEMANTICS`, so a
//! directory junction or a folder at the name opens too (without it the open fails, access
//! denied, before the handle can say what it is); then a regular disk file with one link and no
//! more bytes than its cap, all checked on the handle the bytes are read from. The open shares
//! only reading, so no one writes the file while it is read.
#![expect(
    unsafe_code,
    reason = "FFI to CreateFileW, ReadFile and the handle's file information"
)]

use std::path::Path;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, GENERIC_READ, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ, FILE_TYPE_DISK,
    FileAttributeTagInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, GetFileType,
    OPEN_EXISTING, ReadFile,
};
use windows::core::HSTRING;

use super::SourceError;

/// An open file handle, closed on drop.
pub(super) struct Handle(pub(super) HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful CreateFileW and is closed once, here.
        if let Err(error) = unsafe { CloseHandle(self.0) } {
            tracing::debug!(code = error.code().0, "CloseHandle failed");
        }
    }
}

/// The Win32 code a failed call carries.
pub(super) fn code_of(error: &windows::core::Error) -> u32 {
    u32::try_from(error.code().0 & 0xFFFF).unwrap_or(u32::MAX)
}

/// `call` failed with `error`, as a [`SourceError`].
fn os(call: &'static str, error: &windows::core::Error) -> SourceError {
    SourceError::Os {
        call,
        code: code_of(error),
    }
}

/// The bytes of the file at `path`, read as the module says, at most `cap` of them.
pub(super) fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>, SourceError> {
    let file = open(path)?;
    let size = checked(&file)?;
    if size > cap {
        return Err(SourceError::TooLarge(cap));
    }
    read_all(&file, cap)
}

/// `path` opened for reading, sharing only reading, a reparse point opened as itself and a
/// folder opened as one, for [`checked`] to refuse by what the handle says.
fn open(path: &Path) -> Result<Handle, SourceError> {
    let name = HSTRING::from(path.as_os_str());
    // Without `FILE_FLAG_BACKUP_SEMANTICS`, CreateFileW opens a non-directory only, and a
    // junction at the name (a directory) fails with access denied, not refused by its tag. The
    // flag only lets a directory open here: it overrides an access check only for a token with
    // `SeBackupPrivilege` enabled, and the refresh never enables it (`refresh::run`, its own
    // mode of the exe, enables no privilege). The access asked for stays reading.
    // SAFETY: `name` is NUL-terminated and outlives the call; no security attributes or
    // template are passed. The handle is owned by `Handle` at once.
    let opened = unsafe {
        CreateFileW(
            &name,
            GENERIC_READ.0,
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_SEQUENTIAL_SCAN,
            None,
        )
    };
    match opened {
        Ok(handle) => Ok(Handle(handle)),
        Err(error) => {
            let code = code_of(&error);
            if code == ERROR_FILE_NOT_FOUND.0 || code == ERROR_PATH_NOT_FOUND.0 {
                Err(SourceError::Missing)
            } else {
                Err(os("CreateFileW", &error))
            }
        }
    }
}

/// The open file's size, once it is known to be a regular disk file with one link that isn't
/// a reparse point.
pub(super) fn checked(file: &Handle) -> Result<u64, SourceError> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle is open and `tag` is valid and writable for its size.
    unsafe {
        GetFileInformationByHandleEx(
            file.0,
            FileAttributeTagInfo,
            (&raw mut tag).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0),
        )
    }
    .map_err(|error| os("GetFileInformationByHandleEx", &error))?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 || tag.ReparseTag != 0 {
        return Err(SourceError::ReparsePoint(tag.ReparseTag));
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
        return Err(SourceError::NotRegular);
    }
    // SAFETY: the handle is open.
    if unsafe { GetFileType(file.0) } != FILE_TYPE_DISK {
        return Err(SourceError::NotRegular);
    }
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open and `info` is a live out-pointer.
    unsafe { GetFileInformationByHandle(file.0, &raw mut info) }
        .map_err(|error| os("GetFileInformationByHandle", &error))?;
    if info.nNumberOfLinks != 1 {
        return Err(SourceError::Linked(info.nNumberOfLinks));
    }
    Ok(u64::from(info.nFileSizeHigh) << 32 | u64::from(info.nFileSizeLow))
}

/// Every byte of the open file, refused past `cap` (the size can change between the check and
/// the read, so the read is bounded too).
pub(super) fn read_all(file: &Handle, cap: u64) -> Result<Vec<u8>, SourceError> {
    let limit = usize::try_from(cap).unwrap_or(usize::MAX);
    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let mut read = 0u32;
        // SAFETY: the handle is open for reading; `chunk` is live and writable for its length,
        // and `read` is a live out-pointer. No OVERLAPPED: the handle is synchronous.
        unsafe { ReadFile(file.0, Some(&mut chunk), Some(&raw mut read), None) }
            .map_err(|error| os("ReadFile", &error))?;
        let read = usize::try_from(read).unwrap_or(usize::MAX);
        if read == 0 {
            return Ok(bytes);
        }
        let got = chunk.get(..read).ok_or(SourceError::TooLarge(cap))?;
        if bytes.len() + got.len() > limit {
            return Err(SourceError::TooLarge(cap));
        }
        bytes.extend_from_slice(got);
    }
}

#[cfg(test)]
mod tests;
