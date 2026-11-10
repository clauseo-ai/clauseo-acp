//! A link's target, read from the link's own handle and never followed (rule 8): the link is
//! opened relative to its folder's handle as itself (`FILE_OPEN_REPARSE_POINT`,
//! `OBJ_DONT_REPARSE`), never waiting on an oplock (`FILE_COMPLETE_IF_OPLOCKED`), and its reparse
//! data read with `FSCTL_GET_REPARSE_POINT`. Not `std::fs::read_link`, which opens the full path
//! and so follows any link on the way to it: a symbolic link to a network share partway would make
//! the lawyer's process sign in to that share.
//!
//! A junction's target, and an absolute symbolic link's, is kept as an NT name (`\??\C:\...`,
//! `\??\UNC\server\share`); a relative symbolic link's as the words it was made with. Either
//! is returned as kept, its substitute name, for `scan/locate.rs` to judge by its words.
#![expect(
    unsafe_code,
    reason = "FFI to read a link's reparse data through a handle the set-aside opened as itself"
)]

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::PathBuf;

use windows::Wdk::Storage::FileSystem::{
    FILE_COMPLETE_IF_OPLOCKED, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows::Win32::Storage::FileSystem::{
    FILE_READ_ATTRIBUTES, MAXIMUM_REPARSE_DATA_BUFFER_SIZE, SYNCHRONIZE,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
use windows::Win32::System::SystemServices::{IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK};

use super::read::unblocked;
use super::{Handle, open_at_status, os_error};

/// Where a junction's names start in its reparse data: after the tag, the lengths and the four
/// name offsets and lengths.
const JUNCTION_NAMES: usize = 16;

/// Where a symbolic link's names start: after a junction's fields and the link's flags.
const SYMLINK_NAMES: usize = 20;

/// See [`Volume::link_target`](super::super::run::Volume::link_target).
pub(super) fn link_target(folder: &Handle, name: &OsStr) -> io::Result<PathBuf> {
    let wide: Vec<u16> = name.encode_wide().collect();
    let access = FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;
    let options = FILE_SYNCHRONOUS_IO_NONALERT | FILE_COMPLETE_IF_OPLOCKED;
    let (link, status, information) = open_at_status(folder, &wide, access, FILE_OPEN, options)?;
    unblocked(status, information)?;
    let size = MAXIMUM_REPARSE_DATA_BUFFER_SIZE;
    // u32 words, so the buffer is aligned for the reparse data's header.
    let mut buffer = vec![0_u32; usize::try_from(size / 4).map_err(io::Error::other)?];
    let mut returned = 0_u32;
    // SAFETY: the handle is open for synchronous I/O; `buffer` is writable for `size` bytes and
    // `returned` is a live out-pointer; no input buffer and no overlapped structure.
    unsafe {
        DeviceIoControl(
            link.0,
            FSCTL_GET_REPARSE_POINT,
            None,
            0,
            Some(buffer.as_mut_ptr().cast()),
            size,
            Some(&raw mut returned),
            None,
        )
    }
    .map_err(|e| os_error(&e))?;
    let bytes: Vec<u8> = buffer.iter().flat_map(|word| word.to_le_bytes()).collect();
    let length = usize::try_from(returned).map_err(io::Error::other)?;
    substitute(bytes.get(..length).unwrap_or_default())
}

/// The substitute name in the reparse data `bytes` of a junction or symbolic link.
fn substitute(bytes: &[u8]) -> io::Result<PathBuf> {
    let short = || io::Error::new(io::ErrorKind::InvalidData, "short reparse data");
    let u16_at = |at: usize| -> io::Result<usize> {
        let pair: [u8; 2] = bytes
            .get(at..at + 2)
            .and_then(|pair| pair.try_into().ok())
            .ok_or_else(short)?;
        Ok(usize::from(u16::from_le_bytes(pair)))
    };
    let tag = bytes
        .get(..4)
        .and_then(|four| four.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(short)?;
    let names = match tag {
        IO_REPARSE_TAG_MOUNT_POINT => JUNCTION_NAMES,
        IO_REPARSE_TAG_SYMLINK => SYMLINK_NAMES,
        _ => return Err(io::Error::other("not a junction or a symbolic link")),
    };
    let (offset, length) = (u16_at(8)?, u16_at(10)?);
    let start = names + offset;
    let raw = bytes.get(start..start + length).ok_or_else(short)?;
    let wide: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}
