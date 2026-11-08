//! The Git Bash stub's placement: the runner's exe and its `.sig` read ([`read_signed`]) and,
//! once the signature is checked (`git_tree::stub`), those same bytes written into `git\_off\`
//! as `bash.exe` ([`place_bytes`]). The runner's folder chain is opened plainly (no reparse
//! point at any level), each file opened as itself relative to its folder's handle and read
//! through that handle; the copy is made under a temporary name relative to `_off`'s handle,
//! owner SYSTEM, its DACL inherited, flushed, the old stub renamed aside, then renamed in.
#![expect(unsafe_code, reason = "FFI to FlushFileBuffers")]

use std::mem::offset_of;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_RENAME_INFORMATION, FILE_SEQUENTIAL_ONLY,
    FileRenameInformation,
};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_WRITE_DATA, FileAttributeTagInfo, FlushFileBuffers,
};

use super::super::Refused;
use super::delete::dispose;
use super::files::{info, open_plain_folder, other, plain_chain, refused};
use super::nt::{Descriptor, OWNER_SYSTEM, Open, set_information};
use super::put::{read_all, write_all};
use crate::windows::acl::object::{Handle, os_error, words};
use crate::windows::acl::privilege::{BACKUP_RESTORE, Privileges};

/// The largest runner the stub is copied from.
const CAP: u64 = 256 * 1024 * 1024;
/// The largest `.sig` read: 4 KiB.
const SIG_CAP: u64 = 4096;

/// The regular file `from` and its `<from>.sig`, each opened as itself relative to their
/// folder's handle (the folder's chain opened plainly, no reparse point at any level) and read
/// through that one handle (`TreeFs::read_signed`): the bytes checked are the bytes copied.
pub(super) fn read_signed(from: &Path) -> Result<(Vec<u8>, Vec<u8>), Refused> {
    let (Some(parent), Some(file)) = (from.parent(), from.file_name()) else {
        return Err(other(from, "has no folder"));
    };
    plain_chain(parent)?;
    let above = open_plain_folder(parent)?;
    let name: Vec<u16> = file.encode_wide().collect();
    let mut sig_name = name.clone();
    sig_name.extend(".sig".encode_utf16());
    let exe = read_as_itself(from, above.handle(), &name, CAP)?;
    let sig = read_as_itself(from, above.handle(), &sig_name, SIG_CAP)?;
    Ok((exe, sig))
}

/// The regular file `name` in the folder open as `folder`, opened as itself relative to it and
/// read whole, at most `cap` bytes; `from` names it in errors.
fn read_as_itself(
    from: &Path,
    folder: &Handle,
    name: &[u16],
    cap: u64,
) -> Result<Vec<u8>, Refused> {
    let source = Open {
        access: FILE_READ_DATA.0 | FILE_READ_ATTRIBUTES.0,
        share: FILE_SHARE_READ | FILE_SHARE_DELETE,
        disposition: FILE_OPEN,
        options: FILE_NON_DIRECTORY_FILE,
    }
    .at(folder, name, None)
    .map_err(|error| refused(from, &error))?;
    let fail = |why: &dyn std::fmt::Display| other(from, &why.to_string());
    let tag: FILE_ATTRIBUTE_TAG_INFO =
        info(&source, FileAttributeTagInfo).map_err(|error| fail(&error))?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Refused::Reparse);
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0 {
        return Err(fail(&"is a folder"));
    }
    read_all(&source, cap).map_err(|error| fail(&error))
}

/// Writes `bytes` into the folder `dir` as `temp`, made relative to its handle, owner SYSTEM,
/// its DACL inherited, flushed; sets what is at `name` aside as `<name>.<aside>.old` (a running
/// stub can be renamed, never replaced), then renames `temp` to `name` (`TreeFs::place_bytes`).
/// A failure after `temp` is made deletes it.
pub(super) fn place_bytes(
    bytes: &[u8],
    dir: &Path,
    temp: &str,
    name: &str,
    aside: &str,
) -> Result<(), Refused> {
    let _held = Privileges::enable(&BACKUP_RESTORE)
        .map_err(|error| other(dir, &format!("enabling backup and restore: {error}")))?;
    let owner = Descriptor::from_sddl(OWNER_SYSTEM).map_err(|why| other(dir, &why))?;
    let folder = open_plain_folder(dir)?;
    let temp: Vec<u16> = temp.encode_utf16().collect();
    let target = Open {
        access: FILE_WRITE_DATA.0 | FILE_READ_ATTRIBUTES.0 | DELETE.0,
        share: FILE_SHARE_MODE(0),
        disposition: FILE_CREATE,
        options: FILE_NON_DIRECTORY_FILE | FILE_SEQUENTIAL_ONLY,
    }
    .at(folder.handle(), &temp, Some(&owner))
    .map_err(|error| other(dir, &format!("making the stub's copy: {error}")))?;
    let name: Vec<u16> = name.encode_utf16().collect();
    let mut old = name.clone();
    old.extend(format!(".{aside}.old").encode_utf16());
    let placed = write_all(&target, bytes)
        .and_then(|()| flush(&target))
        .and_then(|()| set_aside(folder.handle(), &name, &old))
        .and_then(|()| rename_in(&target, folder.handle(), &name));
    if let Err(error) = placed {
        let _gone = dispose(&target);
        return Err(other(dir, &format!("placing the stub: {error}")));
    }
    Ok(())
}

/// Renames `name` in the folder open as `folder` to `old` there, opened as itself; nothing
/// there is fine.
fn set_aside(folder: &Handle, name: &[u16], old: &[u16]) -> std::io::Result<()> {
    let found = Open {
        access: DELETE.0 | FILE_READ_ATTRIBUTES.0,
        share: FILE_SHARE_READ | FILE_SHARE_DELETE | FILE_SHARE_WRITE,
        disposition: FILE_OPEN,
        options: FILE_NON_DIRECTORY_FILE,
    }
    .at(folder, name, None);
    match found {
        Ok(file) => rename_in(&file, folder, old),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Writes what `file` holds through to the disk; `file` is open with `FILE_WRITE_DATA`.
fn flush(file: &Handle) -> std::io::Result<()> {
    // SAFETY: the handle is open with FILE_WRITE_DATA.
    unsafe { FlushFileBuffers(file.0) }.map_err(|error| os_error(&error))
}

/// Renames what `file` is open on (with `DELETE`) to `name` in the folder `folder`, relative to
/// its handle, replacing a file there (`FileRenameInformation`, `ReplaceIfExists`). Laid out as
/// `put.rs`'s link: by the fields' offsets in an 8-byte-aligned buffer.
fn rename_in(file: &Handle, folder: &Handle, name: &[u16]) -> std::io::Result<()> {
    let at = offset_of!(FILE_RENAME_INFORMATION, FileName);
    let length = at + name.len() * 2;
    let mut bytes = vec![0_u8; length.max(size_of::<FILE_RENAME_INFORMATION>())];
    let mut put = |offset: usize, data: &[u8]| {
        if let Some(slot) = bytes.get_mut(offset..offset + data.len()) {
            slot.copy_from_slice(data);
        }
    };
    // `ReplaceIfExists`, the union's first byte.
    put(offset_of!(FILE_RENAME_INFORMATION, Anonymous), &[1]);
    put(
        offset_of!(FILE_RENAME_INFORMATION, RootDirectory),
        &folder.0.0.addr().to_ne_bytes(),
    );
    let name_bytes = u32::try_from(name.len() * 2).map_err(std::io::Error::other)?;
    put(
        offset_of!(FILE_RENAME_INFORMATION, FileNameLength),
        &name_bytes.to_ne_bytes(),
    );
    let wide: Vec<u8> = name.iter().flat_map(|unit| unit.to_ne_bytes()).collect();
    put(at, &wide);
    set_information(file, FileRenameInformation, &words(&bytes), length)
}
