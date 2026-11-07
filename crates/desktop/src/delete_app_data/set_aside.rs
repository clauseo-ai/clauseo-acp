//! The set-aside at the end of the way down with `--sandbox-removed`
//! (`<data>\sandbox\set-aside`): kept whole when it holds anything of the lawyer's, deleted when
//! it holds only Clauseo's own bookkeeping. Every tier-B launch leaves `record.json` and
//! `set-aside.lock` in `set-aside\<matter>\` ([`SET_ASIDE_BOOKKEEPING`]); counting those as the
//! lawyer's files would show the Kept box to every lawyer who used tier B.
//!
//! It counts as empty when each entry in it is a real folder (no reparse point) holding only
//! regular files (no reparse point, no folder) named exactly `record.json` or `set-aside.lock`.
//! Anything else (a pass folder with moved files, a manifest, any other name, a link, a folder
//! under a bookkeeping name, an entry that cannot be listed or read) keeps the whole set-aside,
//! untouched. Every folder looked into is pinned first (`ops::pin_folder`: refused if a reparse
//! point, then held so it cannot be renamed, deleted or turned into a link) and stays pinned
//! until its files are gone. Each file goes by `remove_file`, each emptied folder by
//! `remove_dir`, which removes only an empty folder: nothing here recurses, and nothing outside
//! the set-aside is named.

use std::ffi::OsStr;
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_sandbox::windows::SET_ASIDE_BOOKKEEPING;

use super::ops;
use super::walk::{Left, Report, is_link};

/// What the set-aside `path`, listed as a real folder, leaves, as the module says: itself kept
/// when it holds anything but bookkeeping; nothing when the bookkeeping and the emptied folders
/// went; held when one of them could not go (reported), so nothing above it is removed.
pub(super) fn clear(path: PathBuf, report: &mut Report<'_>) -> Left {
    let Some(found) = bookkeeping_only(&path) else {
        return Left {
            kept: Some(path),
            held: false,
        };
    };
    let mut held = false;
    for file in &found.files {
        held |= failed(file, ops::remove_file(file), report);
    }
    // Unpinned only now, so each folder can go.
    drop(found.pins);
    for matter in &found.matters {
        held |= failed(matter, ops::remove_dir(matter), report);
    }
    if !held {
        held = failed(&path, ops::remove_dir(&path), report);
    }
    Left { kept: None, held }
}

/// A set-aside holding only bookkeeping, while its folders are pinned.
struct Found {
    /// The set-aside's pin and each matter's folder's, held until the files are gone.
    pins: Vec<ops::Pinned>,
    /// The bookkeeping files.
    files: Vec<PathBuf>,
    /// The matters' folders.
    matters: Vec<PathBuf>,
}

/// The set-aside `path`, pinned and found to hold only bookkeeping; `None` when anything else
/// is there or anything could not be pinned, listed or read.
fn bookkeeping_only(path: &Path) -> Option<Found> {
    let mut found = Found {
        pins: vec![ops::pin_folder(path).ok()?],
        files: Vec::new(),
        matters: Vec::new(),
    };
    for (matter, meta) in listing(path)? {
        if is_link(&meta) || !meta.is_dir() {
            return None;
        }
        found.pins.push(ops::pin_folder(&matter).ok()?);
        for (file, meta) in listing(&matter)? {
            if !is_bookkeeping(&file, &meta) {
                return None;
            }
            found.files.push(file);
        }
        found.matters.push(matter);
    }
    Some(found)
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
