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
//! - The live watch's reads go the same way: an item is looked at relative to its folder's
//!   handle as a probe opens it, with what it is read from that handle; a folder's own name is
//!   read from its normalized final path (its long name, however it was opened); and a walk
//!   down from a volume's top opens only a drive's root by its path (`C:\`), never a share.
//! - A final path is compared with the planned one part by part, as names are compared.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use windows::Wdk::Storage::FileSystem::{
    FILE_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_OPTIONS,
};
use windows::Win32::Foundation::ERROR_NO_MORE_FILES;
use windows::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
    FILE_TRAVERSE, FileAttributeTagInfo, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
    GETFINALPATHNAMEBYHANDLE_FLAGS, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
    SYNCHRONIZE, VOLUME_NAME_DOS,
};

use super::super::files::FileId;
use super::super::scan::{Kind, Listed};
use super::{Handle, id_of, open_at, open_path, os_error, tag, wide_os};
use crate::windows::acl_plan::verbatim;

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

/// See [`Volume::open_top`](super::super::run::Volume::open_top): a drive's root (`C:\`) alone,
/// checked by its words before anything is opened, so no share or device is ever opened here.
pub(super) fn open_top(top: &Path) -> io::Result<Handle> {
    let drive = matches!(
        top.as_os_str().as_encoded_bytes(),
        [letter, b':', b'\\'] if letter.is_ascii_alphabetic()
    );
    if !drive {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a drive's root",
        ));
    }
    open_root(top)
}

/// See [`Volume::look`](super::super::run::Volume::look): opened as [`probe`] opens it, with
/// what it is read from that handle.
pub(super) fn look(folder: &Handle, name: &OsStr) -> io::Result<Option<(FileId, Kind)>> {
    let name: Vec<u16> = name.encode_wide().collect();
    let options = NTCREATEFILE_CREATE_OPTIONS(0);
    match open_at(folder, &name, PROBE, FILE_OPEN, options) {
        Ok(item) => Ok(Some((id_of(&item)?, kind_of(&item)?))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// What the handle's file, folder or link is, as a listing says it ([`listed`]): a name
/// surrogate is a link, else a folder by its attribute, else anything else.
fn kind_of(handle: &Handle) -> io::Result<Kind> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle is open and `info` is valid and writable for its size.
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileAttributeTagInfo,
            (&raw mut info).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0),
        )
    }
    .map_err(|e| os_error(&e))?;
    let reparse = info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
    Ok(if reparse && surrogate(Some(info.ReparseTag)) {
        Kind::Link
    } else if info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
        Kind::Folder
    } else {
        Kind::Other
    })
}

/// See [`Volume::folder_name`](super::super::run::Volume::folder_name): the last part of its
/// normalized final path, which holds each folder's long name.
pub(super) fn folder_name(folder: &Handle) -> io::Result<OsString> {
    let path = PathBuf::from(OsString::from_wide(&final_path(folder)?));
    path.file_name()
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other("a volume's root has no name"))
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

/// Whether two names are the same as NTFS matches names: `CompareStringOrdinal` with case
/// ignored, so no name is folded here. Text or not: compared as UTF-16.
pub(in crate::windows::set_aside) fn same_name(a: &OsStr, b: &OsStr) -> bool {
    let a: Vec<u16> = a.encode_wide().collect();
    let b: Vec<u16> = b.encode_wide().collect();
    same_wide(&a, &b)
}

/// [`same_name`] on UTF-16.
pub(super) fn same_wide(a: &[u16], b: &[u16]) -> bool {
    // SAFETY: both slices are valid for their lengths, and the call only reads them.
    unsafe { CompareStringOrdinal(a, b, true) == CSTR_EQUAL }
}

/// Whether the handle's final path (normalized, with its drive letter) is `planned`, compared
/// part by part as NTFS compares names ([`same_name`]), both sides in the verbatim form
/// ([`verbatim`]): `dunce`'s plain form kept a final path past `MAX_PATH`, or with a name that
/// isn't Unicode, verbatim while the planned path was plain (round 7, VM run 3).
pub(super) fn same_path(handle: &Handle, planned: &Path) -> io::Result<bool> {
    let found = verbatim(&PathBuf::from(OsString::from_wide(&final_path(handle)?)));
    let planned = verbatim(&std::path::absolute(planned)?);
    let (mut a, mut b) = (found.components(), planned.components());
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ok(true),
            (Some(x), Some(y)) if same_wide(&wide_os(x.as_os_str()), &wide_os(y.as_os_str())) => {}
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
