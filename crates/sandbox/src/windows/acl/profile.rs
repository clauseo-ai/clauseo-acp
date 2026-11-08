//! A pool user's profile folders, reached by handle (ADR 0049, "Reparse points"): elevated
//! set-up and removal never resolve a path inside a profile through a reparse point they didn't
//! check.
//!
//! [`open_below`] opens the profile by the path `ProfileList` gives, as itself
//! (`FILE_FLAG_OPEN_REPARSE_POINT`), and refuses it unless it is no reparse point and its final
//! path is that path. Each part below it (`AppData`, then `LocalLow`) is opened relative to the
//! folder before it (`FILE_OPEN_REPARSE_POINT` and `OBJ_DONT_REPARSE`) and refused if it is a
//! reparse point of any kind; the last one's final path is checked again. [`clear_links`] takes
//! out, before `DeleteProfileW`, every junction, symbolic link and mount point in a profile, and
//! every name of a file that has another name elsewhere, each deleted itself on its own handle,
//! so the profile service's delete meets none of them.
#![expect(
    unsafe_code,
    reason = "FFI to SetFileInformationByHandle, which marks a link itself for deletion"
)]

use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Component, Path};

use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX,
    FILE_DISPOSITION_INFO_EX_FLAGS, FILE_LIST_DIRECTORY, FileDispositionInfoEx,
    SetFileInformationByHandle,
};

use super::object::{Child, Object, os_error, reparse_refused};
use super::same_path;
use crate::windows::acl_bytes::{owner_descriptor, owner_sid, sid_bytes};
use crate::windows::sids::Sid;
use windows::Win32::Security::OWNER_SECURITY_INFORMATION;

/// Why a folder in a profile wasn't reached.
#[derive(Debug)]
pub(crate) enum Below {
    /// It, or a folder on the way to it, isn't there.
    Missing,
    /// This part (`the profile folder`, or a part's name) is a reparse point, or its final path
    /// isn't the planned one: nothing went through it.
    Changed(String),
    /// Anything else.
    Io(io::Error),
}

/// What [`clear_links`] deleted.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cleared {
    /// Junctions, symbolic links and mount points, each deleted itself.
    pub(crate) links: usize,
    /// Names of files with another name elsewhere: the name deleted, the file kept.
    pub(crate) linked_files: usize,
}

/// The profile folder's name in a [`Below::Changed`].
const PROFILE: &str = "the profile folder";

/// The folder `folder`, which is `profile` or below it, opened by handle as the module says:
/// with `WRITE_DAC | WRITE_OWNER` where the handle can have them, as [`Object::open`] asks.
pub(crate) fn open_below(profile: &Path, folder: &Path) -> Result<Object, Below> {
    let parts = parts_below(profile, folder)?;
    let mut at = match Object::open_folder(profile) {
        Ok(at) => at,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Below::Missing),
        Err(e) => return Err(Below::Io(e)),
    };
    if at.reparse() || !same_path(&at, profile).map_err(Below::Io)? {
        return Err(Below::Changed(PROFILE.to_owned()));
    }
    for part in &parts {
        let name: Vec<u16> = part.encode_wide().collect();
        let changed = || Below::Changed(part.to_string_lossy().into_owned());
        at = match at.open_child(&name, true) {
            Ok(next) if next.reparse() => return Err(changed()),
            Ok(next) => next,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Below::Missing),
            Err(e) if reparse_refused(&e) => return Err(changed()),
            Err(e) => return Err(Below::Io(e)),
        };
    }
    if !same_path(&at, folder).map_err(Below::Io)? {
        let last = parts.last().map_or_else(
            || PROFILE.to_owned(),
            |part| part.to_string_lossy().into_owned(),
        );
        return Err(Below::Changed(last));
    }
    Ok(at)
}

/// The owner of `folder`, read on the handle [`open_below`] reaches it by; `None` when it, or a
/// folder on the way, is missing.
pub(crate) fn owner_below(profile: &Path, folder: &Path) -> Result<Option<Sid>, Below> {
    let object = match open_below(profile, folder) {
        Ok(object) => object,
        Err(Below::Missing) => return Ok(None),
        Err(other) => return Err(other),
    };
    let bytes = object
        .security(OWNER_SECURITY_INFORMATION.0)
        .map_err(Below::Io)?;
    owner_sid(&bytes)
        .map(Some)
        .ok_or_else(|| Below::Io(io::Error::new(io::ErrorKind::InvalidData, "no owner")))
}

/// Makes `owner` the owner of `folder` alone (`OWNER_SECURITY_INFORMATION` only, through
/// `SetKernelObjectSecurity` on its handle, so nothing below it changes), reached from `profile`
/// by [`open_below`]: the set-up's hand-over of a pool user's `LocalLow` to `Administrators`
/// (S15 `PR4h`). A handle without `WRITE_OWNER` is refused, untouched.
pub(crate) fn set_owner_below(profile: &Path, folder: &Path, owner: &Sid) -> Result<(), Below> {
    let object = open_below(profile, folder)?;
    if !object.write_owner {
        return Err(Below::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "its owner can't be changed by this process",
        )));
    }
    object
        .set(
            OWNER_SECURITY_INFORMATION,
            &owner_descriptor(&sid_bytes(owner)),
        )
        .map_err(Below::Io)
}

/// Deletes, in the profile `profile` (reached by [`open_below`]), every junction, symbolic link
/// and mount point itself, never what it names, and every name of a file with another name
/// elsewhere, keeping the file and its other names, owner and DACL. Each folder is entered by
/// its handle, never through a link. Any item it can't open stops it, so nothing it didn't see
/// is left to the profile service. An elevated caller enables the backup and restore privileges
/// first, as `setup/profile.rs` does, so a folder its owner shut is still listed.
pub(crate) fn clear_links(profile: &Path) -> Result<Cleared, Below> {
    let root = open_below(profile, profile)?;
    let listed = root.children().map_err(Below::Io)?;
    let mut cleared = Cleared::default();
    let mut stack = vec![(root, listed.into_iter())];
    while let Some((folder, children)) = stack.last_mut() {
        let Some(child) = children.next() else {
            stack.pop();
            continue;
        };
        if let Some(sub) = clear_one(folder, &child, &mut cleared)? {
            let listed = sub.children().map_err(Below::Io)?;
            stack.push((sub, listed.into_iter()));
        }
    }
    Ok(cleared)
}

/// One item of [`clear_links`]: a link or a linked file deleted itself; a folder (not a link)
/// given back, open to be listed; anything else left.
fn clear_one(
    folder: &Object,
    child: &Child,
    cleared: &mut Cleared,
) -> Result<Option<Object>, Below> {
    let list = if child.folder {
        FILE_LIST_DIRECTORY.0
    } else {
        0
    };
    let item = match folder.open_child_for(&child.name, list) {
        Ok(item) => item,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Below::Io(e)),
    };
    let link = item.name_surrogate();
    let linked_file = !item.folder && item.links > 1;
    if !link && !linked_file {
        return Ok(item.folder.then_some(item));
    }
    // Opened again with DELETE only now, so a file in use elsewhere is never asked for it.
    let doomed = folder
        .open_child_for(&child.name, DELETE.0)
        .map_err(Below::Io)?;
    if doomed.file_id().map_err(Below::Io)? != item.file_id().map_err(Below::Io)? {
        return Err(Below::Changed(String::from_utf16_lossy(&child.name)));
    }
    mark_deleted(&doomed).map_err(Below::Io)?;
    if link {
        cleared.links += 1;
    } else {
        cleared.linked_files += 1;
    }
    Ok(None)
}

/// Marks the name `object` is open on for deletion (`FileDispositionInfoEx`: POSIX semantics,
/// read-only ignored), so that name goes when its handle closes: a link itself, never what it
/// names; one name of a file with others, which keep the file as it is.
fn mark_deleted(object: &Object) -> io::Result<()> {
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_INFO_EX_FLAGS(
            FILE_DISPOSITION_FLAG_DELETE.0
                | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0
                | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE.0,
        ),
    };
    // SAFETY: the handle is open with DELETE; `info` is a live FILE_DISPOSITION_INFO_EX of the
    // size given.
    unsafe {
        SetFileInformationByHandle(
            object.handle().0,
            FileDispositionInfoEx,
            (&raw const info).cast(),
            u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).unwrap_or(0),
        )
    }
    .map_err(|e| os_error(&e))
}

/// The parts of `folder` below `profile`, each a plain name; none when they are the same.
fn parts_below<'a>(profile: &Path, folder: &'a Path) -> Result<Vec<&'a std::ffi::OsStr>, Below> {
    let rest = folder.strip_prefix(profile).map_err(|error| {
        Below::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the folder isn't in the profile: {error}"),
        ))
    })?;
    rest.components()
        .map(|part| match part {
            Component::Normal(name) => Ok(name),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => Err(Below::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a part below the profile isn't a plain name",
            ))),
        })
        .collect()
}

#[cfg(test)]
mod tests;
