//! The Windows half of the pack read: entries examined without following a reparse point, and
//! files and folders opened by handles whose attributes, link counts and final paths are
//! checked. `std` has no stable call for the link count or the final path, so these are Win32
//! calls.

#![expect(
    unsafe_code,
    reason = "Win32 calls (CreateFileW, GetFileInformationByHandle, GetFinalPathNameByHandleW) to open a pack's files without following a reparse point and check the handle"
)]

use std::fs::File;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_NAME_NORMALIZED, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    GETFINALPATHNAMEBYHANDLE_FLAGS, GetFileInformationByHandle, GetFinalPathNameByHandleW,
    OPEN_EXISTING, VOLUME_NAME_DOS,
};
use windows::core::PCWSTR;

use super::read::{Kind, check_root, is_reparse};
use super::{Refusal, refuse};

/// A folder of the pack, by its path, with the pack folder's final path as its handle saw
/// it, which every file's final path must extend.
pub(super) struct Folder {
    path: PathBuf,
    root: Rc<[u16]>,
}

impl Folder {
    /// The pack's own folder: a folder, no link and no reparse point, by its path and by its
    /// handle.
    pub(super) fn open_root(folder: &Path) -> Result<Self, Refusal> {
        check_root(folder)?;
        let handle = open(folder)?;
        let info = information(&handle)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(Refusal::Link);
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
            return Err(Refusal::Missing);
        }
        Ok(Self {
            path: folder.to_owned(),
            root: Rc::from(final_path(&handle).ok_or(Refusal::Io)?),
        })
    }

    /// The names in the folder. A name that is not UTF-8 is refused.
    pub(super) fn names(&self) -> Result<Vec<String>, Refusal> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&self.path).map_err(refuse(Refusal::Io))? {
            let entry = entry.map_err(refuse(Refusal::Io))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(refuse(Refusal::BadPath))?;
            names.push(name);
        }
        Ok(names)
    }

    /// What `name` is, from `symlink_metadata`: any reparse point is a link. The link count is
    /// read from the handle at the open.
    pub(super) fn kind(&self, name: &str) -> Result<Kind, Refusal> {
        let meta = std::fs::symlink_metadata(self.path.join(name)).map_err(refuse(Refusal::Io))?;
        Ok(if meta.file_type().is_symlink() || is_reparse(&meta) {
            Kind::Link
        } else if meta.is_dir() {
            Kind::Folder
        } else if meta.is_file() {
            Kind::File {
                len: meta.len(),
                hard_linked: false,
            }
        } else {
            Kind::Other
        })
    }

    /// The folder `name` (`rel` inside the pack), checked by its handle: a folder, no reparse
    /// point, and a final path equal to the pack's own, a backslash and `rel`. A folder
    /// swapped for a junction later is still caught at each file's open.
    pub(super) fn open_folder(&self, name: &str, rel: &str) -> Result<Self, Refusal> {
        let path = self.path.join(name);
        let handle = open(&path)?;
        let info = information(&handle)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            || final_path(&handle).as_deref() != Some(self.expected(rel).as_slice())
        {
            return Err(Refusal::Link);
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
            return Err(Refusal::NotAFile);
        }
        Ok(Self {
            path,
            root: Rc::clone(&self.root),
        })
    }

    /// The final path `rel` must have: the pack folder's, a backslash, then `rel` with a backslash
    /// for each `/`.
    fn expected(&self, rel: &str) -> Vec<u16> {
        let mut expected = self.root.to_vec();
        expected.push(u16::from(b'\\'));
        expected.extend(rel.encode_utf16().map(|unit| {
            if unit == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                unit
            }
        }));
        expected
    }

    /// Opens the file `name` (`rel` inside the pack) without following a reparse point at it,
    /// and refuses it when the handle shows a reparse point, a folder or more than one hard
    /// link, or when its final path is not the pack's own, `\` and `rel`: then a folder on the
    /// way was swapped for a junction after the walk saw it. Never an exec bit on Windows.
    pub(super) fn open_file(&self, name: &str, rel: &str) -> Result<(File, bool), Refusal> {
        let handle = open(&self.path.join(name))?;
        let info = information(&handle)?;
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(Refusal::Link);
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
            return Err(Refusal::NotAFile);
        }
        if info.nNumberOfLinks > 1 {
            return Err(Refusal::HardLink);
        }
        if final_path(&handle).as_deref() != Some(self.expected(rel).as_slice()) {
            return Err(Refusal::Link);
        }
        Ok((File::from(handle), false))
    }
}

/// `path` opened for reading, never through a reparse point at it
/// (`FILE_FLAG_OPEN_REPARSE_POINT`), a folder too (`FILE_FLAG_BACKUP_SEMANTICS`), sharing read,
/// write and delete so no other program is blocked.
fn open(path: &Path) -> Result<OwnedHandle, Refusal> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is NUL-terminated and outlives the call; no security attributes or
    // template are passed.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            FILE_GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(refuse(Refusal::Io))?;
    // SAFETY: `CreateFileW` succeeded, so `handle` is an open handle that nothing else owns;
    // `OwnedHandle` closes it once.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// The handle's attributes, link count and size.
fn information(handle: &OwnedHandle) -> Result<BY_HANDLE_FILE_INFORMATION, Refusal> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open for the borrow, and `info` is a valid, writable struct.
    unsafe { GetFileInformationByHandle(HANDLE(handle.as_raw_handle()), &raw mut info) }
        .map_err(refuse(Refusal::Io))?;
    Ok(info)
}

/// The handle's normalized final path, `\\?\C:\...`, without its NUL; `None` when the call
/// fails, which refuses the pack.
fn final_path(handle: &OwnedHandle) -> Option<Vec<u16>> {
    let flags = GETFINALPATHNAMEBYHANDLE_FLAGS(FILE_NAME_NORMALIZED.0 | VOLUME_NAME_DOS.0);
    let mut buffer = vec![0u16; 512];
    loop {
        // SAFETY: the handle is open for the borrow, and `buffer` is a writable slice of the
        // length the call is given.
        let length = unsafe {
            GetFinalPathNameByHandleW(HANDLE(handle.as_raw_handle()), &mut buffer, flags)
        };
        match usize::try_from(length).ok()? {
            0 => return None,
            // Within the buffer: the path without its NUL. Longer: the size needed, NUL
            // included.
            n if n < buffer.len() => {
                buffer.truncate(n);
                return Some(buffer);
            }
            n => buffer.resize(n + 1, 0),
        }
    }
}
