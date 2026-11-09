//! The lawyer's set-aside folder on a matter's own volume, when that isn't the data folder's
//! (`<volume root>\.clauseo-set-aside-<SID>`, `set_aside::drive_folder_name`), and the folders
//! in it, with their own DACL ([`drive_sddl`]). The folders in it are made and secured by the
//! sandbox folder's mechanics ([`create_as`], [`protect_as`]). The drive folder itself is made
//! with its DACL in place and owned by the lawyer, or, when there, secured through one handle:
//! refused if a link or not a folder, and refused, never taken back, when another account owns
//! it (`set_aside::not_yours`), so a squatter's folder stays theirs until `SYSTEM` sets it
//! aside by its handle (`heal.rs`, [`heal_drive_folders`]).

#![expect(
    unsafe_code,
    reason = "the drive folder made with its DACL in place, and its volume's flags read"
)]

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{
    CreateDirectoryW, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, GetVolumeInformationW,
};
use windows::Win32::System::SystemServices::{FILE_PERSISTENT_ACLS, FILE_READ_ONLY_VOLUME};
use windows::core::{BOOL, PCWSTR};

use super::{Descriptor, Handle, PROTECTED_DACL, create_as, named, os_error, protect_as, wide};
use crate::windows::record::{current_user_sid, sandbox_group_sid};
use crate::windows::set_aside::not_yours;
use crate::windows::sids::Sid;

mod heal;

pub(crate) use heal::heal_drive_folders;

/// The DACL of a set-aside folder on a matter's own volume, for the desktop user `user`:
/// protected, full access for `user` and `SYSTEM` alone, inherited by what is made inside, and
/// first a deny of everything to the pool group `pool_group` (`ClauseoSandboxUsers`) when the
/// machine's record names one. No `Administrators`: the folder sits at a volume's root, outside
/// the data folder, and holds only what was taken from the agent.
pub(crate) fn drive_sddl(user: &str, pool_group: Option<&str>) -> String {
    let deny = pool_group
        .map(|group| format!("(D;OICI;FA;;;{group})"))
        .unwrap_or_default();
    format!("D:P{deny}(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)")
}

/// Makes the folder `path` (its parent must exist) with [`drive_sddl`]'s DACL in place, or
/// secures the one there. The pool group comes from the machine's record
/// (`sandbox_group_sid`), checked to be a SID; a record that can't be read refuses, and none (no
/// set-up) leaves the deny out. A folder in the drive folder (`top` false) is made or secured as
/// [`super::create`] and [`super::protect`] do the sandbox folder. The drive folder (`top`)
/// goes through [`make_top`], after a check of its volume: one that is read-only or keeps no
/// ACLs (FAT, exFAT) can't hold a private folder, and is refused.
pub(crate) fn make_drive_folder(path: &Path, top: bool) -> io::Result<()> {
    let group = pool_group()?;
    let dacl = |user: &str| drive_sddl(user, group.as_ref().map(Sid::as_str));
    if top {
        let volume = path
            .parent()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        check_volume(volume)?;
        let user = current_user_sid()?;
        return make_top(path, &user, &dacl(user.as_str()));
    }
    if !create_as(path, dacl)? {
        protect_as(path, dacl)?;
    }
    Ok(())
}

/// The pool group's SID from the machine's record, checked; `None` when the record names none.
fn pool_group() -> io::Result<Option<Sid>> {
    match sandbox_group_sid() {
        Ok(Some(text)) => Sid::parse(&text)
            .map(Some)
            .map_err(|_bad| io::Error::new(io::ErrorKind::InvalidData, "a bad group SID")),
        Ok(None) => Ok(None),
        Err(error) => Err(io::Error::other(error.to_string())),
    }
}

/// The drive folder `path` for `user`, with the protected DACL `sddl`: made with it in place
/// (`CreateDirectoryW` with the descriptor), or the one there opened once, without following a
/// link, and everything after through that handle. A link or a file is refused. One `user`
/// doesn't own is refused with `not_yours`, never taken back, unless this call made it: an
/// elevated token makes `Administrators` the owner of what it makes, which is set to `user`.
/// Then the DACL is written again on the folder alone, as [`super::protect`] does, so what is
/// made inside inherits it.
fn make_top(path: &Path, user: &Sid, sddl: &str) -> io::Result<()> {
    let made = make_new(path, sddl)?;
    let handle =
        Handle::open(path).map_err(|error| named(path, "can't be opened to secure", &error))?;
    let info = handle
        .information()
        .map_err(|error| named(path, "can't be read", &error))?;
    let refuse = |why: &str| io::Error::new(io::ErrorKind::InvalidData, named_text(path, why));
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(refuse("is a junction or a link"));
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
        return Err(refuse("is not a folder"));
    }
    let owner = handle
        .owner()
        .map_err(|error| named(path, "has an owner that can't be read", &error))?;
    if owner != *user {
        if !made {
            return Err(not_yours());
        }
        handle
            .set_owner(user)
            .map_err(|error| named(path, "can't be given its owner", &error))?;
    }
    let descriptor = Descriptor::protected(sddl)?;
    handle
        .set(PROTECTED_DACL, &descriptor)
        .map_err(|error| named(path, "its DACL can't be set", &error))
}

/// `path` and `why`, as one message.
fn named_text(path: &Path, why: &str) -> String {
    format!("{} {why}", path.display())
}

/// Makes the folder `path` with the descriptor `sddl` in place (an owner too, when it names
/// one, which needs the restore privilege for anyone but the caller). `Ok(false)` when
/// something is there already.
fn make_new(path: &Path, sddl: &str) -> io::Result<bool> {
    let descriptor = Descriptor::from_sddl(sddl)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: BOOL(0),
    };
    let target = wide(path);
    // SAFETY: `target` is NUL-terminated and outlives the call; `attributes` and the descriptor
    // it points to live until this function returns.
    match unsafe { CreateDirectoryW(PCWSTR(target.as_ptr()), Some(&raw const attributes)) } {
        Ok(()) => Ok(true),
        Err(error) if error.code() == ERROR_ALREADY_EXISTS.to_hresult() => Ok(false),
        Err(error) => Err(os_error(&error)),
    }
}

/// Refuses the volume whose root is `root` when it is read-only or keeps no ACLs.
fn check_volume(root: &Path) -> io::Result<()> {
    let mut wide: Vec<u16> = root.as_os_str().encode_wide().collect();
    if wide.last() != Some(&u16::from(b'\\')) {
        wide.push(u16::from(b'\\'));
    }
    wide.push(0);
    let mut flags = 0_u32;
    // SAFETY: `wide` is NUL-terminated and outlives the call; `flags` is a live, writable u32,
    // and the buffers are left out.
    unsafe {
        GetVolumeInformationW(
            PCWSTR(wide.as_ptr()),
            None,
            None,
            None,
            Some(&raw mut flags),
            None,
        )
    }
    .map_err(|error| os_error(&error))?;
    if flags & FILE_READ_ONLY_VOLUME != 0 {
        return Err(io::Error::new(
            io::ErrorKind::ReadOnlyFilesystem,
            "the matter's drive is read-only",
        ));
    }
    if flags & FILE_PERSISTENT_ACLS == 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the matter's drive keeps no permissions",
        ));
    }
    Ok(())
}
