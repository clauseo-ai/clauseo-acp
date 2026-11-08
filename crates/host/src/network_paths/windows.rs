//! Windows: what a drive letter names, by `QueryDosDeviceW` on the letter alone (never
//! `GetDriveTypeW`, which can open a disconnected mapped drive and wait on it); a reparse point's
//! tag, from its folder's listing (`FindFirstFileW`), never by opening it; and a local file's
//! identity, opened without following a link and only once the walk has vouched for its path.
#![expect(
    unsafe_code,
    reason = "FFI to QueryDosDeviceW, FindFirstFileW, GetShortPathNameW and file identity"
)]

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdInfo, FindClose, FindFirstFileW,
    GetFileInformationByHandleEx, GetShortPathNameW, OPEN_EXISTING, QueryDosDeviceW,
    WIN32_FIND_DATAW,
};
use windows::core::{HSTRING, PCWSTR};

use super::windows_names::drive_is_local;

/// Room for a device name (`QueryDosDeviceW` writes each target NUL-terminated, then a NUL).
const DEVICE_ROOM: usize = 1024;

/// Whether drive `letter` is remote: it does not lead to a local volume
/// ([`drive_is_local`], a `subst` followed), or it can't be asked.
pub(super) fn remote_drive(letter: char) -> bool {
    !drive_is_local(letter, &device_of)
}

/// The first device target `QueryDosDeviceW` names for drive `letter`, `None` when it can't.
fn device_of(letter: char) -> Option<String> {
    let name = HSTRING::from(format!("{letter}:"));
    let mut target = vec![0_u16; DEVICE_ROOM];
    // SAFETY: `name` is a NUL-terminated UTF-16 string, `target` a buffer of its length; the
    // call writes at most that many characters and keeps no pointer to either.
    let written = unsafe { QueryDosDeviceW(&name, Some(&mut target)) };
    if written == 0 {
        return None;
    }
    let end = target.iter().position(|&c| c == 0).unwrap_or(target.len());
    Some(String::from_utf16_lossy(
        target.get(..end).unwrap_or_default(),
    ))
}

/// `path` as a NUL-terminated UTF-16 string.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// The reparse tag of the reparse point at `path`, from its folder's listing: `FindFirstFileW`
/// reads the entry, never opens it. `None` when it can't be read.
pub(super) fn reparse_tag(path: &Path) -> Option<u32> {
    let path = wide(path);
    let mut data = WIN32_FIND_DATAW::default();
    // SAFETY: `path` is NUL-terminated and outlives the call; `data` is written in place.
    let found = unsafe { FindFirstFileW(PCWSTR(path.as_ptr()), &raw mut data) }.ok()?;
    // SAFETY: `found` is the search handle just opened, closed once.
    let _closed = unsafe { FindClose(found) };
    Some(data.dwReserved0)
}

/// The identity of the local file or folder at `path`: its volume's serial number and its
/// 128-bit file ID, opened with no access, without following a link at it.
pub(super) fn identity(path: &Path) -> std::io::Result<(u64, u128)> {
    let path = wide(path);
    // SAFETY: `path` is NUL-terminated and outlives the call; no access is asked, every share
    // mode is allowed, and a link at the path is opened itself, never followed.
    let handle: HANDLE = unsafe {
        CreateFileW(
            PCWSTR(path.as_ptr()),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            None,
        )
    }
    .map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
    let mut info = FILE_ID_INFO::default();
    let size = u32::try_from(size_of::<FILE_ID_INFO>()).unwrap_or(0);
    // SAFETY: `handle` is open; `info` is a FILE_ID_INFO of `size` bytes written in place.
    let read =
        unsafe { GetFileInformationByHandleEx(handle, FileIdInfo, (&raw mut info).cast(), size) };
    // SAFETY: `handle` was opened above and is closed once.
    let _closed = unsafe { CloseHandle(handle) };
    read.map_err(|error| std::io::Error::from_raw_os_error(error.code().0 & 0xFFFF))?;
    Ok((
        info.VolumeSerialNumber,
        u128::from_le_bytes(info.FileId.Identifier),
    ))
}

/// The 8.3 short form of the local path `path` (`GetShortPathNameW`), asked only of the data
/// folder once it is proven local. `None` when it can't be read.
pub(crate) fn short_path(path: &Path) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;
    let path = wide(path);
    let mut room = vec![0_u16; 512];
    for _ in 0..2 {
        // SAFETY: `path` is NUL-terminated and outlives the call; `room` is written in place,
        // at most its length.
        let written = unsafe { GetShortPathNameW(PCWSTR(path.as_ptr()), Some(&mut room)) };
        let written = usize::try_from(written).ok()?;
        if written == 0 {
            return None;
        }
        if written < room.len() {
            let short = room.get(..written)?;
            return Some(std::ffi::OsString::from_wide(short).into());
        }
        room.resize(written.saturating_add(1), 0);
    }
    None
}
