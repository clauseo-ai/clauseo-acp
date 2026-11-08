//! The set-aside at the end of the way down with `--sandbox-removed`
//! (`<data>\sandbox\set-aside`): kept whole when it holds anything of the lawyer's, deleted when
//! it holds only Clauseo's own bookkeeping. Every tier-B launch leaves `record.json` and
//! `set-aside.lock` in `set-aside\<matter>\`, a launch with a notes folder leaves
//! `notes-record.json`, and a pass that set something aside leaves `manifest.json`, which a
//! restore of every item empties ([`SET_ASIDE_BOOKKEEPING`]); counting those as the lawyer's
//! files would show the Kept box to every lawyer who used tier B.
//!
//! It counts as empty when each entry in it is a real folder (no reparse point) holding only
//! regular files (no reparse point, no folder) named exactly `record.json`,
//! `notes-record.json`, `set-aside.lock` or `manifest.json`. Anything else (a pass folder with moved files, any other name, a name with a
//! trailing dot or space, a link, a folder under a bookkeeping name, an entry that cannot be
//! listed or read) keeps the whole set-aside, untouched. Each folder is pinned while it is listed
//! (`ops::pin_folder`: refused if a reparse point).
//!
//! The deletion itself is the sandbox crate's (`forget_set_aside_bookkeeping`), one matter at a
//! time and by handle, never through a link: the set-aside opened as itself, the matter's folder
//! and each file relative to it, each refused if a reparse point, all checked before any is
//! deleted; a record that still lists an open session, a manifest that still lists an item set
//! aside, either unreadable, or a file with another hard link, leaves that matter's folder as it
//! is, and the set-aside is then kept. The pins are closed first, since they share reads
//! only. Last, the emptied set-aside goes by `remove_dir` by its path, which removes only an
//! empty folder, or a link put in its place as the link alone; nothing here recurses.

use std::ffi::OsStr;
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_sandbox::windows::{
    SET_ASIDE_BOOKKEEPING, SetAsideBookkeeping, forget_set_aside_bookkeeping,
};

use super::ops;
use super::walk::{Left, Report, is_link};

/// What the set-aside `path`, listed as a real folder, leaves, as the module says: itself kept
/// when it holds anything but bookkeeping, or a matter's folder stayed; nothing when the
/// bookkeeping and the emptied folders went; held when one of them could not go (reported), so
/// nothing above it is removed.
pub(super) fn clear(path: PathBuf, report: &mut Report<'_>) -> Left {
    let Some(matters) = bookkeeping_only(&path) else {
        return Left {
            kept: Some(path),
            held: false,
        };
    };
    let (mut left, mut held) = (false, false);
    for matter in &matters {
        let Some(name) = matter.file_name() else {
            continue;
        };
        match forget_set_aside_bookkeeping(&path, name) {
            Ok(SetAsideBookkeeping::Gone | SetAsideBookkeeping::NoFolder) => {}
            Ok(SetAsideBookkeeping::Left) => left = true,
            Err(failure) => {
                report.fail(matter, &io::Error::other(failure));
                held = true;
            }
        }
    }
    if left {
        return Left {
            kept: Some(path),
            held,
        };
    }
    if !held {
        held = failed(&path, ops::remove_dir(&path), report);
    }
    Left { kept: None, held }
}

/// The matters' folders in the set-aside `path`, when it holds only bookkeeping; `None` when
/// anything else is there or anything could not be pinned, listed or read. Each pin is closed
/// on return.
fn bookkeeping_only(path: &Path) -> Option<Vec<PathBuf>> {
    let _pinned = ops::pin_folder(path).ok()?;
    let mut matters = Vec::new();
    for (matter, meta) in listing(path)? {
        if is_link(&meta) || !meta.is_dir() {
            return None;
        }
        let _pinned = ops::pin_folder(&matter).ok()?;
        for (file, meta) in listing(&matter)? {
            if !is_bookkeeping(&file, &meta) {
                return None;
            }
        }
        matters.push(matter);
    }
    Some(matters)
}

/// Whether `path` is a bookkeeping file: a regular file, no reparse point, named exactly as one.
fn is_bookkeeping(path: &Path, meta: &Metadata) -> bool {
    !is_link(meta)
        && meta.is_file()
        && path
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| SET_ASIDE_BOOKKEEPING.contains(&name))
}

/// `folder`'s entries with their own metadata (a link's, never its target's); `None` when the
/// folder or any entry cannot be listed or read.
fn listing(folder: &Path) -> Option<Vec<(PathBuf, Metadata)>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).ok()? {
        let path = entry.ok()?.path();
        let meta = std::fs::symlink_metadata(&path).ok()?;
        found.push((path, meta));
    }
    Some(found)
}

/// Whether `result` failed at `path`, reported; a path already gone is no failure.
fn failed(path: &Path, result: io::Result<()>, report: &mut Report<'_>) -> bool {
    match result {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            report.fail(path, &error);
            true
        }
        _ => false,
    }
}
