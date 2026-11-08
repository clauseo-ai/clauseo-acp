//! A Git install, copy or tree walked ([`walk`]): each folder listed through its own handle
//! (`Object::children`), each entry opened relative to that handle as itself
//! (`OBJ_DONT_REPARSE`, `FILE_OPEN_REPARSE_POINT`) and described from the handle it was opened
//! by: its kind, file ID, size, write time, and owner and DACL. A reparse point is listed and
//! never entered.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use windows::Win32::Security::{DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION};
use windows::Win32::Storage::FileSystem::{
    FILE_BASIC_INFO, FILE_LIST_DIRECTORY, FILE_STANDARD_INFO, FileBasicInfo, FileStandardInfo,
    READ_CONTROL,
};

use super::super::{Entry, Refused};
use super::files::{info, open_plain_folder, other, refused};
use crate::windows::acl::object::Object;
use crate::windows::acl::privilege::Privileges;
use crate::windows::admin_copy::level_safe;

/// Every folder, file and reparse point below the folder `root`, each folder before what it
/// holds, as the module says. `root` a reparse point is [`Refused::Reparse`].
pub(super) fn walk(root: &Path) -> Result<Vec<Entry>, Refused> {
    // Backup intent with the privilege reads what a DACL shuts Administrators out of; without
    // it, such an entry fails to open, which fails the walk.
    let _backup = Privileges::enable(&["SeBackupPrivilege"]).ok();
    let folder = open_plain_folder(root)?;
    let mut out = Vec::new();
    walk_in(root, &folder, Path::new(""), &mut out)?;
    Ok(out)
}

/// [`walk`] below the open folder `folder`, whose path from the root is `rel`.
fn walk_in(root: &Path, folder: &Object, rel: &Path, out: &mut Vec<Entry>) -> Result<(), Refused> {
    let children = folder.children().map_err(|error| refused(root, &error))?;
    for child in children {
        let path = rel.join(OsString::from_wide(&child.name));
        // The right to list it only for what the listing calls a plain folder; what the handle
        // says decides what it is.
        let list = if child.folder && !child.reparse {
            FILE_LIST_DIRECTORY.0
        } else {
            0
        };
        let opened = folder
            .open_child_for(&child.name, READ_CONTROL.0 | list)
            .map_err(|error| refused(root, &error))?;
        let entry = described(root, &opened, path.clone())?;
        let enter = entry.folder;
        out.push(entry);
        if enter {
            if list == 0 {
                return Err(other(root, "changed while it was listed"));
            }
            walk_in(root, &opened, &path, out)?;
        }
    }
    Ok(())
}

/// The entry `rel` of the walk of `root`, from the handle `object` opened it by: a reparse
/// point is never a folder; a folder's file ID and size are 0.
fn described(root: &Path, object: &Object, rel: PathBuf) -> Result<Entry, Refused> {
    let fail = |what: &str, error: io::Error| other(root, &format!("reading {what}: {error}"));
    let bytes = object
        .security((OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION).0)
        .map_err(|error| fail("an entry's security", error))?;
    let basic: FILE_BASIC_INFO =
        info(object.handle(), FileBasicInfo).map_err(|error| fail("an entry's times", error))?;
    let reparse = object.reparse();
    let folder = object.folder && !reparse;
    let (id, size) = if folder {
        (0, 0)
    } else {
        let (_volume, id) = object
            .file_id()
            .map_err(|error| fail("a file's ID", error))?;
        let standard: FILE_STANDARD_INFO = info(object.handle(), FileStandardInfo)
            .map_err(|error| fail("a file's size", error))?;
        (
            u128::from_le_bytes(id),
            u64::try_from(standard.EndOfFile).unwrap_or(0),
        )
    };
    Ok(Entry {
        rel,
        folder,
        reparse,
        id,
        size,
        written: u64::try_from(basic.LastWriteTime).unwrap_or(0),
        admin_only: level_safe(&bytes),
    })
}
