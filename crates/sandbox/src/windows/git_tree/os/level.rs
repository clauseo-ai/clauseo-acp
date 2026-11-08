//! A Git tree's folders made and secured ([`level`]): opened, or made, relative to their
//! parent's handle (`nt::Open`: `FILE_OPEN_IF`, `OBJ_DONT_REPARSE`, `FILE_OPEN_REPARSE_POINT`)
//! so no link on the way or at the name is followed, then given their owner and protected DACL
//! through that handle and read back.
//!
//! The descriptor is set with `SetKernelObjectSecurity`, on the folder alone: an account's
//! folder holds trees whose files are hard links, sharing their security with the source
//! copy's files and every other tree's, and a propagating write would rewrite those. The
//! trees' files keep the copy's descriptor; what parts the trees is the folders'.
#![expect(unsafe_code, reason = "FFI to SetKernelObjectSecurity")]

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Wdk::Storage::FileSystem::{FILE_DIRECTORY_FILE, FILE_OPEN_IF};
use windows::Win32::Security::{
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, SetKernelObjectSecurity,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FileAttributeTagInfo, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
};

use super::super::Refused;
use super::files::info;
use super::nt::{Descriptor, OWNER_DACL, Open, code, to_sddl};
use crate::windows::acl::object::{Handle, Object, kernel_security, reparse_refused, words};
use crate::windows::acl::privilege::Privileges;
use crate::windows::admin_copy::{Found, os as admin_os};
use crate::windows::refresh::sddl_matches;

/// `ERROR_DIRECTORY`: a file where a folder was asked for.
const NOT_A_FOLDER: i32 = 267;

/// Makes the folder `path` if missing, opened as itself relative to its parent's handle, then
/// gives it the owner and protected DACL `sddl` describes and reads them back. A reparse point
/// at the parent or at `path` is [`Refused::Reparse`]; a file there is refused. Setting another
/// owner than the caller (SYSTEM, as an elevated administrator) needs `SeRestorePrivilege`,
/// enabled for the call, which also opens a folder whose DACL leaves Administrators out.
pub(super) fn level(path: &Path, sddl: &str) -> Result<(), Refused> {
    let fail = |why: &str| Refused::Other(format!("{}: {why}", path.display()));
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(fail("has no parent"));
    };
    let _restore = Privileges::enable(&["SeRestorePrivilege"])
        .map_err(|error| fail(&format!("enabling SeRestorePrivilege: {error}")))?;
    let above = Object::open_folder(parent).map_err(|error| match error {
        e if reparse_refused(&e) => Refused::Reparse,
        e => fail(&format!("opening its parent: {e}")),
    })?;
    if above.reparse() {
        return Err(Refused::Reparse);
    }
    if !above.folder {
        return Err(fail("its parent isn't a folder"));
    }
    let name: Vec<u16> = name.encode_wide().collect();
    let folder = match open_or_make(above.handle(), &name) {
        Ok(folder) => folder,
        Err(error) if reparse_refused(&error) => return Err(Refused::Reparse),
        Err(error) if error.raw_os_error() == Some(NOT_A_FOLDER) => {
            // A file, or a link to one, opened as itself.
            return Err(match admin_os::found(path) {
                Ok(Found::Link) => Refused::Reparse,
                _ => fail("is a file"),
            });
        }
        Err(error) => return Err(fail(&format!("opening it: {error}"))),
    };
    let tag: FILE_ATTRIBUTE_TAG_INFO =
        info(&folder, FileAttributeTagInfo).map_err(|error| fail(&error.to_string()))?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(Refused::Reparse);
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 == 0 {
        return Err(fail("isn't a folder"));
    }
    secure(&folder, sddl).map_err(|why| fail(&why))
}

/// Opens the folder `admin`, then each of `levels`' names below it relative to the one above
/// (`NtCreateFile`, `OBJ_DONT_REPARSE`, `FILE_OPEN_REPARSE_POINT`) for its security and
/// attributes alone, and refuses unless each is a folder and no reparse point whose owner and
/// DACL read back as its SDDL (`refresh::sddl_matches`, so a DACL that isn't protected fails).
/// The outer runner's check of a tree, as the unelevated desktop user: each level's
/// `READ_SECURITY` ACE lets it read them. The words name a level, never a path.
pub(crate) fn checked_chain(admin: &Path, levels: &[(String, String)]) -> Result<(), String> {
    open_chain(admin, levels).map(drop)
}

/// [`checked_chain`], then the file `file` in the last level, opened relative to its handle the
/// same way for its security alone: a regular file and no reparse point. Its owner and DACL
/// (self-relative) are returned for the caller's rule; the outer runner holds the Git Bash
/// stub to `admin_copy::level_safe`.
pub(crate) fn checked_file(
    admin: &Path,
    levels: &[(String, String)],
    file: &str,
) -> Result<Vec<u8>, String> {
    let above = open_chain(admin, levels)?;
    let wide: Vec<u16> = file.encode_utf16().collect();
    let found = match above.open_child_for(&wide, READ_CONTROL.0) {
        Ok(found) => found,
        Err(error) if reparse_refused(&error) => return Err(format!("{file} is a link")),
        Err(error) => return Err(format!("opening {file} failed ({})", kind(&error))),
    };
    if found.reparse() || found.folder {
        return Err(format!("{file} is a link or a folder"));
    }
    kernel_security(found.handle(), OWNER_DACL.0)
        .map_err(|error| format!("reading {file}'s security failed ({})", kind(&error)))
}

/// [`checked_chain`]'s walk, giving the last level open.
fn open_chain(admin: &Path, levels: &[(String, String)]) -> Result<Object, String> {
    let mut above = Object::open_folder(admin)
        .map_err(|error| format!("opening the admin folder failed ({})", kind(&error)))?;
    if above.reparse() || !above.folder {
        return Err("the admin folder is a link or no folder".to_owned());
    }
    for (name, sddl) in levels {
        let wide: Vec<u16> = name.encode_utf16().collect();
        let level = match above.open_child_for(&wide, READ_CONTROL.0) {
            Ok(level) => level,
            Err(error) if reparse_refused(&error) => return Err(format!("{name} is a link")),
            Err(error) => return Err(format!("opening {name} failed ({})", kind(&error))),
        };
        if level.reparse() || !level.folder {
            return Err(format!("{name} is a link or no folder"));
        }
        let back = kernel_security(level.handle(), OWNER_DACL.0)
            .map_err(|error| format!("reading {name}'s security failed ({})", kind(&error)))?;
        let mut back = words(&back);
        let found = to_sddl(PSECURITY_DESCRIPTOR(back.as_mut_ptr().cast()))?;
        if !sddl_matches(&found, &Descriptor::from_sddl(sddl)?.to_sddl()?) {
            return Err(format!("{name}'s owner or DACL isn't set-up's"));
        }
        above = level;
    }
    Ok(above)
}

/// `error`'s code, else its kind.
fn kind(error: &io::Error) -> String {
    error
        .raw_os_error()
        .map_or_else(|| error.kind().to_string(), |code| code.to_string())
}

/// Sets `sddl`'s owner and protected DACL on what `folder` is open on, then reads them back
/// and refuses unless they are that (`refresh::sddl_matches`).
fn secure(folder: &Handle, sddl: &str) -> Result<(), String> {
    let wanted = Descriptor::from_sddl(sddl)?;
    let info = OWNER_DACL | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: the handle is open with WRITE_DAC and WRITE_OWNER; the descriptor is a valid,
    // self-relative one, alive for the call.
    unsafe { SetKernelObjectSecurity(folder.0, info, wanted.0) }
        .map_err(|error| format!("SetKernelObjectSecurity failed ({})", code(&error)))?;
    let back = kernel_security(folder, OWNER_DACL.0)
        .map_err(|error| format!("reading the security back: {error}"))?;
    let mut back = words(&back);
    let found = to_sddl(PSECURITY_DESCRIPTOR(back.as_mut_ptr().cast()))?;
    if !sddl_matches(&found, &wanted.to_sddl()?) {
        // Neither descriptor is given: either names a SID.
        return Err("the folder's security reads back changed".to_owned());
    }
    Ok(())
}

/// Opens the folder `name` in the folder `dir` relative to its handle, making it when missing
/// (inheriting `dir`'s DACL until [`secure`] replaces it), with what [`secure`] needs. A
/// reparse point at `name` is opened itself; one met on the way fails the open.
fn open_or_make(dir: &Handle, name: &[u16]) -> io::Result<Handle> {
    Open {
        access: READ_CONTROL.0 | WRITE_DAC.0 | WRITE_OWNER.0 | FILE_READ_ATTRIBUTES.0,
        share: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        disposition: FILE_OPEN_IF,
        options: FILE_DIRECTORY_FILE,
    }
    .at(dir, name, None)
}
