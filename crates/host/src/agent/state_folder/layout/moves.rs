//! The moves of `layout.rs` (R17 PR-D1): every rename is one no-replace rename on one volume
//! (`write::rename_no_replace`), so nothing at the new name is ever replaced, and a folder is
//! at its old name or its new one, never split by a crash.
//!
//! A folder that holds a link moves entry by entry, never whole: a relative link resolves
//! against the folder it sits in, and the notes' move takes them one level up
//! (`agent-state/<m>/notes` to `notes/<m>`), so a link `../../agents/…` would come to name the
//! login store. Whether a folder holds one is a bounded scan ([`may_hold_links`]); past its caps
//! the folder counts as holding one. Entry by entry, each link stays behind in the old folder,
//! and each entry that holds none moves whole. The entries go into a staging folder beside the
//! new one ([`staging`]), which is renamed to the new name last, so the new name appears only
//! complete: a launch never sees half of a matter's notes, and a start cut short goes on into
//! the same staging folder at the next start.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// The most entries [`may_hold_links`] reads before it counts the folder as holding a link.
const SCAN_ENTRIES: usize = 10_000;

/// The deepest folder level [`may_hold_links`] reads, and the deepest an entry-by-entry move
/// goes; below it a folder counts as holding a link, and an entry there stays where it is.
const SCAN_DEPTH: usize = 32;

/// The one rename of a move or a merge ([`crate::write::rename_no_replace`]). In unit tests a
/// failure of a chosen kind can stand in for it, at once or after some renames
/// ([`super::tests::fail_renames_after`]): a move across volumes, a start cut short.
pub(super) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(test)]
    if let Some(kind) = super::tests::next_rename_fails() {
        return Err(io::Error::from(kind));
    }
    crate::write::rename_no_replace(from, to)
}

/// Syncs the folders holding `old` and `new` after a move, so the renames are on disk (Unix;
/// Windows has nothing to sync).
pub(super) fn sync_parents(old: &Path, new: &Path) {
    sync(&[old.parent(), new.parent()]);
}

/// Syncs each folder of `folders` (Unix). Not needed for safety (a rename a power cut loses
/// leaves the old name, which the next start moves again), so a failure is only logged.
fn sync(folders: &[Option<&Path>]) {
    for folder in folders.iter().flatten() {
        if let Err(error) = crate::write::sync_dir(folder) {
            tracing::debug!(
                error = %crate::log_file::Cause(&error),
                "a moved folder's parent was not synced"
            );
        }
    }
}

/// Whether anything below `folder` is a link (or a junction, or any reparse point Windows names
/// a link), read without following one: at most [`SCAN_ENTRIES`] entries and [`SCAN_DEPTH`]
/// levels, past either of which, and on any read failure, the answer is yes.
pub(super) fn may_hold_links(folder: &Path) -> bool {
    let mut budget = SCAN_ENTRIES;
    let mut folders = vec![(folder.to_owned(), 0)];
    while let Some((at, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&at) else {
            return true;
        };
        for entry in entries {
            let Some(kind) = entry.ok().and_then(|entry| {
                let kind = entry.file_type().ok()?;
                Some((entry.path(), kind))
            }) else {
                return true;
            };
            if budget == 0 || kind.1.is_symlink() {
                return true;
            }
            budget -= 1;
            if kind.1.is_dir() {
                if depth + 1 >= SCAN_DEPTH {
                    return true;
                }
                folders.push((kind.0, depth + 1));
            }
        }
    }
    false
}

/// The staging folder of `new`: `.<name>.moving` beside it. Each entry of a folder that holds a
/// link goes in there first; it is renamed to `new` last.
pub(super) fn staging(new: &Path) -> Option<PathBuf> {
    let mut name = OsString::from(".");
    name.push(new.file_name()?);
    name.push(".moving");
    Some(new.with_file_name(name))
}

/// Moves the real folder `old` to `new` (nothing there; its parent made): one rename when
/// `links` is false, or nothing below `old` is a link ([`may_hold_links`]) and no staging folder
/// is there; else entry by entry into [`staging`] (made, or found from a start cut short, whose
/// entries must not be left behind), the links left in `old`, then the staging folder renamed to
/// `new`, then `old` removed when empty. Returns how many entries stayed in `old`.
pub(super) fn move_folder(old: &Path, new: &Path, links: bool) -> io::Result<usize> {
    let staged = staging(new)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no folder name"))?;
    let staged_before = std::fs::symlink_metadata(&staged).is_ok();
    if !links || (!staged_before && !may_hold_links(old)) {
        rename(old, new)?;
        sync_parents(old, new);
        return Ok(0);
    }
    let left = move_entries(old, &staged, 0)?;
    match rename(&staged, new) {
        // Another launch's prep gave its staging folder the new name first, and nothing was
        // left for this one: its own empty staging folder goes.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            crate::write::remove_empty_dir(&staged).map_err(|_not_empty| error)?;
        }
        renamed => renamed?,
    }
    sync_parents(old, new);
    if left == 0 {
        crate::write::remove_empty_dir(old)?;
    }
    Ok(left)
}

/// Moves each entry of the real folder `from` into `to` (made when missing, a real folder):
/// a link stays; a folder that holds a link goes on entry by entry, `depth` levels down, to at
/// most [`SCAN_DEPTH`]; any other entry moves whole in one rename. `to` is a staging folder only
/// this move fills, so a folder already there (a start cut short) is one of this move's and is
/// gone on with entry by entry. A folder of `from` emptied below the top is removed. Returns how
/// many entries stayed (links, and folders that still hold them).
fn move_entries(from: &Path, to: &Path, depth: usize) -> io::Result<usize> {
    match crate::write::create_private_dir(to) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        made => made?,
    }
    if !crate::write::is_real_dir(to) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a staging folder is not a folder",
        ));
    }
    let mut left = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let (path, target) = (entry.path(), to.join(entry.file_name()));
        if kind.is_symlink() {
            left += 1;
        } else if kind.is_dir() && (may_hold_links(&path) || crate::write::is_real_dir(&target)) {
            if depth + 1 >= SCAN_DEPTH {
                left += 1;
                continue;
            }
            let below = move_entries(&path, &target, depth + 1)?;
            if below == 0 {
                crate::write::remove_empty_dir(&path)?;
            } else {
                left += 1;
            }
        } else {
            rename(&path, &target)?;
        }
    }
    Ok(left)
}

/// Merges the old notes folder `old` into the new one `new`, which a downgraded app's notes
/// left beside it: each entry of `old` whose name `new` lacks moves in one no-replace rename (a
/// folder whole), so none is ever split or overwritten; a name `new` has already (a clash), a
/// link, and a folder that holds a link ([`may_hold_links`]) stay where they are. `old` is
/// removed once empty. Run at the start only, when no agent runs. A failure stops the merge; the
/// next start goes on from where it stopped. Returns (moved, left).
pub(super) fn merge_notes(old: &Path, new: &Path) -> io::Result<(usize, usize)> {
    let (mut moved, mut left) = (0, 0);
    for entry in std::fs::read_dir(old)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_symlink() || (kind.is_dir() && may_hold_links(&path)) {
            left += 1;
            continue;
        }
        let target = new.join(entry.file_name());
        match rename(&path, &target) {
            Ok(()) => moved += 1,
            // A clash: `AlreadyExists`, or on Windows access denied onto a folder.
            Err(_clash) if std::fs::symlink_metadata(&target).is_ok() => left += 1,
            Err(error) => return Err(error),
        }
    }
    if moved > 0 {
        sync(&[Some(old), Some(new)]);
    }
    if left == 0 {
        crate::write::remove_empty_dir(old)?;
    }
    Ok((moved, left))
}
