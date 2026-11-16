//! The temp files [`super::stage`] makes beside a file (`.clauseo-<n>.tmp`): their names, their
//! identity, their removal, and the sweep of the ones a crash left.
//!
//! A temp file whose removal is refused (macOS: a deny-delete entry it inherited, or the folder's
//! `delete_child` denied; Windows: the same, or the read-only attribute) is unlocked and removed
//! again, but only the one file the write made, known by the handle it was created with: device
//! and inode (Unix), or volume serial and file index (Windows). It is opened again without
//! following a link, and unlocked through that handle only when the handle is that file, a
//! regular file with exactly one link. Never by path: an agent's hard link at a temp name shares
//! its inode, and so its ACL, with a lawyer's file, and a junction there leads a path to another
//! file. The sweep never unlocks anything: it removes what it may, and leaves the rest, which the
//! write-back then names ([`temps_in`]).

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use super::TEMP_TRIES;
use crate::Error;

/// A file's identity: device and inode on Unix, volume serial and file index on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileId {
    pub(super) volume: u64,
    pub(super) index: u64,
}

/// What an open handle says of its file.
#[derive(Debug, Clone, Copy)]
pub(super) struct Identity {
    pub(super) id: FileId,
    /// Its hard links.
    pub(super) links: u64,
    /// A regular file: not a folder, a symlink or a junction (any reparse point on Windows).
    pub(super) regular: bool,
}

/// The identity of the open file `file`, from its handle (`fstat`; Windows:
/// `GetFileInformationByHandle`).
pub(super) fn identity(file: &File) -> io::Result<Identity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let metadata = file.metadata()?;
        Ok(Identity {
            id: FileId {
                volume: metadata.dev(),
                index: metadata.ino(),
            },
            links: metadata.nlink(),
            regular: metadata.file_type().is_file(),
        })
    }
    #[cfg(windows)]
    {
        super::windows::identity(file)
    }
}

/// A new temp file in `dir`, by the first free name `.clauseo-<n>.tmp`, and its identity, taken
/// from the handle that made it (`None` if that fails, or finds anything but a regular file with
/// one link: then it is never unlocked).
pub(super) fn new_temp(dir: &Path) -> Result<(PathBuf, File, Option<FileId>), Error> {
    for n in 0..TEMP_TRIES {
        let path = dir.join(format!(".clauseo-{n}.tmp"));
        match super::create_temp(&path) {
            Ok(file) => {
                // Just made: a regular file with one link, or something is wrong with the folder.
                let id = identity(&file)
                    .ok()
                    .filter(|found| found.regular && found.links == 1)
                    .map(|found| found.id);
                return Ok((path, file, id));
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(Error::io("create", path, err)),
        }
    }
    Err(Error::io(
        "create",
        dir.join(".clauseo-0.tmp"),
        io::ErrorKind::AlreadyExists.into(),
    ))
}

/// Removes the temp file `temp` a write made, whose handle had the identity `id`. When that is
/// refused, the file is unlocked by its handle and removed again ([`unlock_and_remove`]); if
/// that cannot be done, the first refusal is the error.
pub(super) fn remove_staged(temp: &Path, id: Option<FileId>) -> io::Result<()> {
    match super::remove_file(temp) {
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => match id {
            Some(id) if unlock_and_remove(temp, id).is_ok() => Ok(()),
            _ => Err(err),
        },
        removed => removed,
    }
}

/// macOS: opens `temp` without following a symlink; if that handle is the file `id`, with one
/// link, gives it (`acl_set_fd_np`) an ACL letting the current user delete it, then removes the
/// name if it is still that file (`lstat`'s device and inode; macOS has no unlink by
/// descriptor, and a name is all an unlink can change).
#[cfg(target_os = "macos")]
fn unlock_and_remove(temp: &Path, id: FileId) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    let file = super::open_read_nofollow(temp)?;
    only_link(&file, id)?;
    super::macos::allow_delete(&file)?;
    drop(file);
    let now = std::fs::symlink_metadata(temp)?;
    if now.dev() != id.volume || now.ino() != id.index {
        return Err(not_the_temp());
    }
    super::remove_file(temp)
}

/// Windows: opens `temp` without following a reparse point; if that handle is the file `id`,
/// with one link, sets its DACL (`SetSecurityInfo`) to its owner allowed everything and clears
/// its read-only attribute (`SetFileInformationByHandle`), both through the handle; then opens
/// it again for delete, checks it is still that file, and deletes it through that handle.
#[cfg(windows)]
fn unlock_and_remove(temp: &Path, id: FileId) -> io::Result<()> {
    use super::windows;
    let file = windows::open_no_follow(temp, windows::UNLOCK_ACCESS)?;
    only_link(&file, id)?;
    windows::allow_delete(&file)?;
    drop(file);
    let file = windows::open_no_follow(temp, windows::DELETE_ACCESS)?;
    only_link(&file, id)?;
    windows::delete(&file)
}

/// Linux: a mode or an ACL never refuses a removal (that is the folder's permission), so there
/// is nothing to unlock.
#[cfg(not(any(target_os = "macos", windows)))]
fn unlock_and_remove(temp: &Path, id: FileId) -> io::Result<()> {
    let _ = (temp, id);
    Err(io::ErrorKind::Unsupported.into())
}

/// `Ok` when the open `file` is the regular file `id`, with exactly one link.
#[cfg(any(target_os = "macos", windows))]
fn only_link(file: &File, id: FileId) -> io::Result<()> {
    let found = identity(file)?;
    if found.regular && found.links == 1 && found.id == id {
        Ok(())
    } else {
        Err(not_the_temp())
    }
}

/// The error for a name that is no longer the temp file a write made.
#[cfg(any(target_os = "macos", windows))]
fn not_the_temp() -> io::Error {
    io::Error::other("not the temp file this write made")
}

/// Removes the temp files a crash left in `dir` (every `.clauseo-<digits>.tmp`, at any number:
/// a write whose first renames went leaves the later ones), before a write there: the matter's
/// one writer has none in flight then. One listing of `dir`; links and anything not a regular
/// file are skipped. Nothing is unlocked: a file that does not go stays (the write-back names it,
/// [`temps_in`]), and is logged once as a count.
pub(crate) fn sweep_temps(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut left = 0_u32;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !is_temp_name(&name) || !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        #[cfg(test)]
        if super::injected("sweep").is_err() {
            left += 1;
            continue;
        }
        match super::remove_file(&entry.path()) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => left += 1,
            _ => {}
        }
    }
    if left > 0 {
        tracing::warn!(left, "a crashed write's temp files left behind");
    }
}

/// The names of the temp files in `dir` (regular files named as [`is_temp_name`] says), sorted:
/// after a write-back, the ones that could not be removed.
pub(crate) fn temps_in(dir: &Path) -> Vec<OsString> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<OsString> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.file_name())
        .filter(|name| is_temp_name(name))
        .collect();
    names.sort();
    names
}

/// Whether `name` is exactly `.clauseo-<one or more ASCII digits>.tmp`, [`new_temp`]'s names.
pub(super) fn is_temp_name(name: &OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(".clauseo-"))
        .and_then(|rest| rest.strip_suffix(".tmp"))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit()))
}
