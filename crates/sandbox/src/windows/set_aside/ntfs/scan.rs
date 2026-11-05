//! The set-aside scan's calls on NTFS (S15 `PR3b` round 3, R1 R2). Nothing here follows a link or
//! writes:
//!
//! - The matter's folder is opened by its path as itself (`FILE_FLAG_OPEN_REPARSE_POINT`),
//!   refused if a link or if its final path isn't the policy's; each folder below it is opened
//!   relative to its parent's handle (`NtCreateFile` with `RootDirectory`,
//!   `FILE_OPEN_REPARSE_POINT` and `OBJ_DONT_REPARSE`), and refused if it is a link now. Each is
//!   opened to list it and read its attributes, nothing more.
//! - A folder is listed through its handle (`FileIdBothDirectoryInfo`). A name whose attributes
//!   say it is a reparse point with a name-surrogate tag (MS-FSCC puts the tag in `EaSize`) is
//!   a link: a junction or a symbolic link.
//! - A protected name or a link is probed by opening it relative to its folder's handle with
//!   `FILE_READ_ATTRIBUTES` alone, which the folder's `FILE_LIST_DIRECTORY` grants whatever the
//!   item's own DACL says, as itself (`FILE_OPEN_REPARSE_POINT`) and with `OBJ_CASE_INSENSITIVE`,
//!   as git and Claude open names. Its file ID is read from that handle.
//! - Names are compared as NTFS compares them: `CompareStringOrdinal` with case ignored.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::Path;

use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_OPTIONS,
};
use windows::Win32::Foundation::ERROR_NO_MORE_FILES;
use windows::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_TRAVERSE, FileIdBothDirectoryInfo,
    FileIdBothDirectoryRestartInfo, GetFileInformationByHandleEx, SYNCHRONIZE,
};

use super::super::files::FileId;
use super::super::scan::{Kind, Listed};
use super::{Handle, id_of, open_at, open_path, os_error, same_path, tag};

/// `IsReparseTagNameSurrogate`: the tag's name-surrogate bit (junctions, symbolic links).
const NAME_SURROGATE: u32 = 0x2000_0000;

/// What a folder the scan visits is opened with: list it, pass through it to open its names,
/// read its attributes. `SYNCHRONIZE`, since the listing needs synchronous I/O.
const LIST: u32 = FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;

/// What a probe opens a name with: its attributes only, which its folder's
/// `FILE_LIST_DIRECTORY` grants. No `SYNCHRONIZE`, which the item's own DACL would have to give.
const PROBE: u32 = FILE_READ_ATTRIBUTES.0;

/// Whether a reparse tag is a name surrogate.
fn surrogate(tag: Option<u32>) -> bool {
    tag.is_some_and(|tag| tag & NAME_SURROGATE != 0)
}

/// An error for a folder that is a link now, or isn't where it was planned.
fn changed() -> io::Error {
    io::Error::other("the folder is a link now, or moved")
}

/// See [`Volume::open_root`](super::super::run::Volume::open_root).
pub(super) fn open_root(root: &Path) -> io::Result<Handle> {
    let folder = open_path(root, LIST)?;
    if surrogate(tag(&folder)?) || !same_path(&folder, root)? {
        return Err(changed());
    }
    Ok(folder)
}

/// See [`Volume::open_folder`](super::super::run::Volume::open_folder).
pub(super) fn open_folder(folder: &Handle, name: &OsStr) -> io::Result<Handle> {
    let name: Vec<u16> = name.encode_wide().collect();
    let options = FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT;
    let next = open_at(folder, &name, LIST, FILE_OPEN, options)?;
    if surrogate(tag(&next)?) {
        return Err(changed());
    }
    Ok(next)
}

/// See [`Volume::probe`](super::super::run::Volume::probe).
pub(super) fn probe(folder: &Handle, name: &OsStr) -> io::Result<Option<FileId>> {
    let name: Vec<u16> = name.encode_wide().collect();
    let options = NTCREATEFILE_CREATE_OPTIONS(0);
    match open_at(folder, &name, PROBE, FILE_OPEN, options) {
        Ok(item) => id_of(&item).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// See [`Volume::list`](super::super::run::Volume::list).
pub(super) fn list(folder: &Handle) -> io::Result<Vec<Listed>> {
    let mut buffer = vec![0_u64; 8192];
    let size = u32::try_from(buffer.len() * 8).map_err(io::Error::other)?;
    let mut class = FileIdBothDirectoryRestartInfo;
    let mut out = Vec::new();
    loop {
        // SAFETY: the handle is open and `buffer` is writable for `size` bytes.
        let read = unsafe {
            GetFileInformationByHandleEx(folder.0, class, buffer.as_mut_ptr().cast(), size)
        };
        class = FileIdBothDirectoryInfo;
        match read {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => return Ok(out),
            Err(e) => return Err(os_error(&e)),
        }
        // SAFETY: `buffer` is `size` initialised bytes, and u8 has no alignment.
        let bytes =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), size as usize) };
        listed(bytes, &mut out)?;
    }
}

/// Adds the names in one `FILE_ID_BOTH_DIR_INFO` chain, `bytes`, to `out`.
fn listed(bytes: &[u8], out: &mut Vec<Listed>) -> io::Result<()> {
    let attributes_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileAttributes);
    let length_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength);
    let tag_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, EaSize);
    let name_at = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
    let short = || io::Error::new(io::ErrorKind::InvalidData, "a short folder listing");
    let mut at = 0_usize;
    loop {
        let entry = bytes.get(at..).ok_or_else(short)?;
        let next = read_u32(entry, 0).ok_or_else(short)?;
        let attributes = read_u32(entry, attributes_at).ok_or_else(short)?;
        let tag = read_u32(entry, tag_at).ok_or_else(short)?;
        let length = usize::try_from(read_u32(entry, length_at).ok_or_else(short)?)
            .map_err(io::Error::other)?;
        let raw = entry.get(name_at..name_at + length).ok_or_else(short)?;
        let name: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        let dots = name == [u16::from(b'.')] || name == [u16::from(b'.'), u16::from(b'.')];
        if !dots && !name.is_empty() {
            let reparse = attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
            let kind = if reparse && surrogate(Some(tag)) {
                Kind::Link
            } else if attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
                Kind::Folder
            } else {
                Kind::Other
            };
            out.push(Listed {
                name: std::ffi::OsString::from_wide(&name),
                kind,
            });
        }
        if next == 0 {
            return Ok(());
        }
        at += usize::try_from(next).map_err(io::Error::other)?;
    }
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Whether two names, or two keys, are the same as NTFS matches names: `CompareStringOrdinal`
/// with case ignored, so no name is folded here.
pub(in crate::windows::set_aside) fn same_name(a: &str, b: &str) -> bool {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    same_wide(&a, &b)
}

/// [`same_name`] on UTF-16.
pub(super) fn same_wide(a: &[u16], b: &[u16]) -> bool {
    // SAFETY: both slices are valid for their lengths, and the call only reads them.
    unsafe { CompareStringOrdinal(a, b, true) == CSTR_EQUAL }
}
