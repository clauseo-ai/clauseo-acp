//! The chooser's machine on Windows: the drives (`GetLogicalDrives`, `GetDriveTypeW`), the
//! known folders read without a check (`KF_FLAG_DONT_VERIFY`, a registry read), a folder's
//! entries with their attributes and reparse tags in one pass (`FindFirstFileExW`), and the
//! pre-check's probe (`GetFileAttributesW`). The calls that touch a folder may block on a share
//! that is down; the chooser runs each on a thread of its own with a time limit.
#![expect(unsafe_code, reason = "FFI to the Windows file and known-folder APIs")]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt as _;

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_NO_MORE_FILES};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_SYSTEM, FIND_FIRST_EX_FLAGS, FIND_FIRST_EX_LARGE_FETCH, FindClose,
    FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, GetDriveTypeW,
    GetFileAttributesW, GetLogicalDrives, INVALID_FILE_ATTRIBUTES, WIN32_FIND_DATAW,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_RoamingAppData, FOLDERID_Videos, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath,
};
use windows::core::{GUID, HSTRING};

use super::listing::{Entry, Machine};
use super::stall::KnownFolders;

/// The most entries one read takes from a folder, files included, before it stops.
const READ_MAX: usize = 50_000;

/// The known folders Windows' folder dialog shows, which the pre-check looks at.
const SHOWN: [GUID; 7] = [
    FOLDERID_Documents,
    FOLDERID_RoamingAppData,
    FOLDERID_Desktop,
    FOLDERID_Downloads,
    FOLDERID_Pictures,
    FOLDERID_Music,
    FOLDERID_Videos,
];

/// This machine.
pub(super) struct System;

impl Machine for System {
    fn drives(&self) -> Vec<char> {
        // SAFETY: no arguments; returns a bitmask of the drive letters.
        let mask = unsafe { GetLogicalDrives() };
        ('A'..='Z')
            .zip(0..26)
            .filter(|(_, bit)| mask & (1 << bit) != 0)
            .filter(|(letter, _)| matches!(drive_type(*letter), DRIVE_FIXED | DRIVE_REMOVABLE))
            .map(|(letter, _)| letter)
            .collect()
    }

    fn remote_drive(&self, letter: char) -> bool {
        drive_type(letter) == DRIVE_REMOTE
    }

    fn desktop(&self) -> Option<String> {
        known_folder(&FOLDERID_Desktop)
    }

    fn read(&self, folder: &str) -> std::io::Result<Vec<Entry>> {
        let pattern = HSTRING::from(format!("{}\\*", folder.trim_end_matches('\\')));
        let mut data = WIN32_FIND_DATAW::default();
        // SAFETY: `pattern` is NUL-terminated and lives for the call, `data` is a valid
        // WIN32_FIND_DATAW the call fills for `FindExInfoBasic`, and no filter is passed.
        let found = unsafe {
            FindFirstFileExW(
                &pattern,
                FindExInfoBasic,
                (&raw mut data).cast(),
                FindExSearchNameMatch,
                None,
                FIND_FIRST_EX_LARGE_FETCH,
            )
        };
        let handle = found.map_err(|error| {
            if error.code() == ERROR_ACCESS_DENIED.to_hresult() {
                std::io::Error::from(std::io::ErrorKind::PermissionDenied)
            } else {
                std::io::Error::from(std::io::ErrorKind::NotFound)
            }
        })?;
        let mut entries = Vec::new();
        loop {
            entries.push(entry(&data));
            if entries.len() >= READ_MAX {
                break;
            }
            // SAFETY: `handle` is the open search, and `data` a valid buffer for the next entry.
            match unsafe { FindNextFileW(handle, &raw mut data) } {
                Ok(()) => {}
                Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
                Err(_) => break,
            }
        }
        // SAFETY: `handle` is the search FindFirstFileExW opened, closed once here.
        drop(unsafe { FindClose(handle) });
        Ok(entries)
    }

    fn plain_dir(&self, folder: &str) -> bool {
        let attributes = attributes(folder);
        if attributes == INVALID_FILE_ATTRIBUTES || attributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
            return false;
        }
        if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 {
            return true;
        }
        // A reparse point: plain only when its tag is no link (a OneDrive placeholder).
        let path = HSTRING::from(folder.trim_end_matches('\\'));
        let mut data = WIN32_FIND_DATAW::default();
        // SAFETY: as in `read`, for the folder itself rather than its contents.
        let found = unsafe {
            FindFirstFileExW(
                &path,
                FindExInfoBasic,
                (&raw mut data).cast(),
                FindExSearchNameMatch,
                None,
                FIND_FIRST_EX_FLAGS::default(),
            )
        };
        let Ok(handle) = found else {
            return false;
        };
        // SAFETY: `handle` is the search just opened, closed once here.
        drop(unsafe { FindClose(handle) });
        !entry(&data).link
    }
}

impl KnownFolders for System {
    fn paths(&self) -> Vec<String> {
        SHOWN.iter().filter_map(known_folder).collect()
    }

    fn remote_drive(&self, letter: char) -> bool {
        drive_type(letter) == DRIVE_REMOTE
    }

    fn answers(&self, path: &str) -> bool {
        attributes(path) != INVALID_FILE_ATTRIBUTES
    }
}

/// An entry as the chooser reads it; `.` and `..` come out as names it refuses.
fn entry(data: &WIN32_FIND_DATAW) -> Entry {
    let length = data
        .cFileName
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(data.cFileName.len());
    let name = OsString::from_wide(data.cFileName.get(..length).unwrap_or_default());
    let attributes = data.dwFileAttributes;
    // `IsReparseTagNameSurrogate`: bit 29 of the tag, which `dwReserved0` holds for a reparse
    // point.
    let link =
        attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 && data.dwReserved0 & 0x2000_0000 != 0;
    Entry {
        name: name.to_string_lossy().into_owned(),
        dir: attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
        hidden: attributes & (FILE_ATTRIBUTE_HIDDEN.0 | FILE_ATTRIBUTE_SYSTEM.0) != 0,
        link,
    }
}

/// `path`'s attributes, or `INVALID_FILE_ATTRIBUTES`.
fn attributes(path: &str) -> u32 {
    let path = HSTRING::from(path);
    // SAFETY: `path` is NUL-terminated and lives for the call.
    unsafe { GetFileAttributesW(&path) }
}

/// The type of the drive `letter`.
fn drive_type(letter: char) -> u32 {
    let root = HSTRING::from(format!("{letter}:\\"));
    // SAFETY: `root` is NUL-terminated and lives for the call.
    unsafe { GetDriveTypeW(&root) }
}

/// A known folder's path, read without a check (`KF_FLAG_DONT_VERIFY`: no disk or network).
fn known_folder(id: &GUID) -> Option<String> {
    // SAFETY: `id` points to a valid GUID for the whole call, and no access token is passed, so
    // the call reads the current user's folder.
    let path = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY, None) }.ok()?;
    // SAFETY: on success the call returns a NUL-terminated wide string it allocated.
    let folder = OsString::from_wide(unsafe { path.as_wide() });
    // SAFETY: the string came from SHGetKnownFolderPath, which says to free it with
    // CoTaskMemFree, and its only borrow has ended.
    unsafe { CoTaskMemFree(Some(path.as_ptr().cast())) };
    folder.into_string().ok()
}
