//! The set-aside's folder on a matter's own volume, when that isn't the data folder's
//! (`<volume root>\.clauseo-set-aside`, `set_aside::ITEMS_ROOT`), and the folders in it: made
//! and secured by the same mechanics as the sandbox folder ([`create_as`], [`protect_as`]:
//! refused if a link or not a folder, taken back from another owner or refused), with their own
//! DACL ([`drive_sddl`]). The folder's name holds no SID, so a second Windows user of the
//! machine finds the first one's folder there and is refused (a known limit).

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
use windows::Win32::System::SystemServices::{FILE_PERSISTENT_ACLS, FILE_READ_ONLY_VOLUME};
use windows::core::PCWSTR;

use super::{create_as, os_error, protect_as};
use crate::windows::record::sandbox_group_sid;
use crate::windows::sids::Sid;

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
/// secures the one there, as [`super::create`] and [`super::protect`] do the sandbox folder.
/// The pool group comes from the machine's record (`sandbox_group_sid`), checked to be a SID;
/// a record that can't be read refuses, and none (no set-up) leaves the deny out. When `top`,
/// the volume is checked first: one that is read-only or keeps no ACLs (FAT, exFAT) can't hold
/// a private folder, and is refused.
pub(crate) fn make_drive_folder(path: &Path, top: bool) -> io::Result<()> {
    if top {
        let volume = path
            .parent()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        check_volume(volume)?;
    }
    let group = match sandbox_group_sid() {
        Ok(Some(text)) => Some(
            Sid::parse(&text)
                .map_err(|_bad| io::Error::new(io::ErrorKind::InvalidData, "a bad group SID"))?,
        ),
        Ok(None) => None,
        Err(error) => return Err(io::Error::other(error.to_string())),
    };
    let dacl = |user: &str| drive_sddl(user, group.as_ref().map(Sid::as_str));
    if !create_as(path, dacl)? {
        protect_as(path, dacl)?;
    }
    Ok(())
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
