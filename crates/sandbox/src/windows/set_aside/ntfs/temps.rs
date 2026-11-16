//! The temp files a crash left beside a matter's record and manifest, removed under the lock
//! (`files.rs`'s `clear_temps`), by handle and never through a link: the matter's folder opened
//! by its path as itself and refused if a reparse point, listed through that handle, and each
//! name that is exactly a temp `write::write_atomic` gives (`write::is_atomic_temp`) opened
//! relative to it with `DELETE` (`FILE_NON_DIRECTORY_FILE`, so a folder or a junction under the
//! name doesn't open). Marked for deletion only when it is a plain file with one link; anything
//! else stays. Nothing else in the folder is opened.

use std::ffi::OsStr;
use std::io;
use std::path::Path;

use windows::Wdk::Storage::FileSystem::{
    FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT,
};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_TRAVERSE, SYNCHRONIZE,
};

use super::super::SetAsideFailure;
use super::super::files::io_failure;
use super::forget::{links, set_deleted};
use super::scan::list;
use super::{Handle, open_at, open_folder, reparse, wide_os};
use crate::windows::write::is_atomic_temp;

/// What the matter's folder is opened with, to list it and open its names.
const LIST: u32 = FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;

/// What a temp is opened with to mark it for deletion.
const TEMP: u32 = DELETE.0 | FILE_READ_ATTRIBUTES.0 | SYNCHRONIZE.0;

/// Marks for deletion each name in `base` (the matter's folder) that is a temp of one of
/// `targets`, a plain file with one link, as the module says. A name that can't be opened or
/// marked is logged and skipped; the listing is the failure.
pub(in crate::windows::set_aside) fn remove_temps(
    base: &Path,
    targets: &[&OsStr],
) -> Result<(), SetAsideFailure> {
    let folder = open_folder(base, LIST)?;
    for listed in list(&folder).map_err(|error| io_failure(&error))? {
        if !targets
            .iter()
            .any(|target| is_atomic_temp(target, &listed.name))
        {
            continue;
        }
        if let Err(error) = remove_plain(&folder, &listed.name) {
            tracing::debug!(%error, "a set-aside temp stays");
        }
    }
    Ok(())
}

/// Marks `name` in `folder` for deletion when it is a plain file with one link: a link, or a
/// file with another hard link, is left as it is. The mark completes when the handle closes.
fn remove_plain(folder: &Handle, name: &OsStr) -> io::Result<()> {
    let options = FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT;
    let file = open_at(folder, &wide_os(name), TEMP, FILE_OPEN, options)?;
    if reparse(&file)? || links(&file)? != 1 {
        return Ok(());
    }
    set_deleted(&file, true)
}
