//! What the set-aside reads through a handle it opened as itself: an item's owner, from the
//! handle its move renames (`Ntfs::move_into`) or, for the live watch, from one opened relative
//! to the item's folder (`owner_at`), and a small file a scan reads (a `commondir`), each opened
//! so that it never waits on another process's oplock.
//!
//! An agent can hold a batch oplock, or a read-write-handle lease, on a file it made and never
//! acknowledge the break: a plain open for read would then wait with no end, and the watch with
//! it. `FILE_COMPLETE_IF_OPLOCKED` makes the open return at once instead, with
//! `STATUS_OPLOCK_BREAK_IN_PROGRESS` (or `FILE_OPBATCH_BREAK_UNDERWAY` in the status block's
//! `Information`) while a break is under way; that open counts as unreadable, and the handle is
//! closed unread.
#![expect(
    unsafe_code,
    reason = "FFI to read an item's owner and a small file through handles the set-aside opened"
)]

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt as _;

use windows::Wdk::Storage::FileSystem::{
    FILE_COMPLETE_IF_OPLOCKED, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows::Win32::Foundation::{NTSTATUS, STATUS_OPLOCK_BREAK_IN_PROGRESS};
use windows::Win32::Security::OWNER_SECURITY_INFORMATION;
use windows::Win32::Storage::FileSystem::{
    FILE_READ_ATTRIBUTES, FILE_READ_DATA, READ_CONTROL, ReadFile, SYNCHRONIZE,
};

use super::{Handle, open_at_status, os_error, tag};

/// `FILE_OPBATCH_BREAK_UNDERWAY`: what `Information` says when the open found a batch oplock's
/// break under way.
const BREAK_UNDERWAY: usize = 4;

/// The owner of the handle's file or folder, as a SID's text; the handle has `READ_CONTROL`.
pub(super) fn owner_of(handle: &Handle) -> io::Result<String> {
    // Borrowed, never closed here: `handle` closes it.
    let borrowed = std::mem::ManuallyDrop::new(crate::windows::acl::object::Handle(handle.0));
    let bytes =
        crate::windows::acl::object::kernel_security(&borrowed, OWNER_SECURITY_INFORMATION.0)?;
    crate::windows::acl_bytes::owner_sid(&bytes)
        .map(|sid| sid.to_string())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no owner"))
}

/// See [`Volume::owner_at`](super::super::run::Volume::owner_at): `name` opened relative to
/// `folder` as itself (`FILE_OPEN_REPARSE_POINT`, `OBJ_DONT_REPARSE`) with `READ_CONTROL` and
/// `FILE_COMPLETE_IF_OPLOCKED`, its owner read on that handle.
pub(super) fn owner_at(folder: &Handle, name: &OsStr) -> io::Result<String> {
    let wide: Vec<u16> = name.encode_wide().collect();
    let options = FILE_COMPLETE_IF_OPLOCKED;
    let (item, status, information) =
        open_at_status(folder, &wide, READ_CONTROL.0, FILE_OPEN, options)?;
    unblocked(status, information)?;
    owner_of(&item)
}

/// An error when an open with `FILE_COMPLETE_IF_OPLOCKED` found an oplock's break under way
/// (its `status`, or its status block's `information`): the file counts as unreadable.
pub(super) fn unblocked(status: NTSTATUS, information: usize) -> io::Result<()> {
    if status == STATUS_OPLOCK_BREAK_IN_PROGRESS || information == BREAK_UNDERWAY {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "another process holds an oplock on the file",
        ));
    }
    Ok(())
}

/// See [`Volume::read_small`](super::super::run::Volume::read_small): the file `name` opened
/// relative to `folder` as itself (`FILE_OPEN_REPARSE_POINT`, `OBJ_DONT_REPARSE`), refused if a
/// reparse point or a folder, with `FILE_COMPLETE_IF_OPLOCKED`; at most `max` bytes read.
pub(super) fn read_small(folder: &Handle, name: &OsStr, max: u64) -> io::Result<Vec<u8>> {
    let wide: Vec<u16> = name.encode_wide().collect();
    let options =
        FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_COMPLETE_IF_OPLOCKED;
    // `FILE_READ_ATTRIBUTES` for the reparse check below (`FileAttributeTagInfo` needs it);
    // `SYNCHRONIZE` for `FILE_SYNCHRONOUS_IO_NONALERT`.
    let access = FILE_READ_DATA.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;
    let (file, status, information) = open_at_status(folder, &wide, access, FILE_OPEN, options)?;
    unblocked(status, information)?;
    if tag(&file)?.is_some() {
        return Err(io::Error::other("the file is a link"));
    }
    let max = usize::try_from(max).unwrap_or(usize::MAX);
    let mut bytes = vec![0_u8; max];
    let mut filled = 0;
    while filled < max {
        let mut read = 0_u32;
        let rest = bytes.get_mut(filled..).unwrap_or_default();
        // SAFETY: the handle is open for synchronous reads; `rest` is writable for its length
        // and `read` is a live out-pointer; no overlapped structure.
        unsafe { ReadFile(file.0, Some(rest), Some(&raw mut read), None) }
            .map_err(|e| os_error(&e))?;
        let read = usize::try_from(read).unwrap_or(0);
        if read == 0 {
            break;
        }
        filled += read;
    }
    bytes.truncate(filled);
    Ok(bytes)
}
