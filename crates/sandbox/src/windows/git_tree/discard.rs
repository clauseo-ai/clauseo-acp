//! How the Git trees delete anything below `git\` ([`discard`]): a plain delete, never a
//! take-back or a lock-down. Those set a DACL that propagates, and a tree's files are hard
//! links whose security they share with the files they name.
//!
//! What a delete leaves (a file a running process holds: Windows refuses to delete a loaded
//! image, but lets it be renamed) moves into `git\_trash\<random>`: the folder whole, else
//! each file left, one by one. Every run empties the trash ([`empty_trash`]); the removal
//! schedules what is still there for deletion at the next restart. Pure: tested on every OS
//! with the fake.

use std::path::Path;

use super::{TRASH_DIR, TRASH_SDDL, TreeFs, same};

/// What [`discard`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Discarded {
    /// Deleted, or nothing was there.
    Gone,
    /// Moved into the trash, all of it, for a later run to delete.
    Trashed,
    /// Some of it is still at its path.
    Left,
}

/// Deletes `path`, below the trees' folder `git`, by a plain delete; what is left moves into
/// the trash, as the module says. A path in the trash only gets the delete.
pub(super) fn discard(fs: &mut impl TreeFs, git: &Path, path: &Path) -> Discarded {
    let why = match fs.remove_tree(path) {
        Ok(()) => return Discarded::Gone,
        Err(why) => why,
    };
    let trash = git.join(TRASH_DIR);
    if same(path, &trash) || path.parent().is_some_and(|parent| same(parent, &trash)) {
        return Discarded::Left;
    }
    tracing::warn!(%why, "a Git tree's folder couldn't be deleted at once; it moves to the trash");
    if let Err(why) = fs.level(&trash, TRASH_SDDL) {
        tracing::warn!(?why, "the Git trees' trash couldn't be made");
        return Discarded::Left;
    }
    let to = trash.join(fs.unique());
    if fs.rename(path, &to).is_ok() {
        // Most of it may go now; the rest waits for a later run.
        let _later = fs.remove_tree(&to);
        return Discarded::Trashed;
    }
    // A folder doesn't move while a file in it is open; each file left moves on its own.
    if let Ok(left) = fs.walk(path) {
        for entry in left.iter().filter(|entry| !entry.folder) {
            let to = trash.join(fs.unique());
            let _stays = fs.rename(&path.join(&entry.rel), &to);
        }
    }
    match fs.remove_tree(path) {
        Ok(()) => Discarded::Trashed,
        Err(why) => {
            tracing::warn!(%why, "a Git tree's folder stays until a later run");
            Discarded::Left
        }
    }
}

/// Deletes what waits in the trash of the trees' folder `git`; what is still in use stays for
/// a later run. Whether the trash is empty now.
pub(super) fn empty_trash(fs: &mut impl TreeFs, git: &Path) -> bool {
    let trash = git.join(TRASH_DIR);
    let mut empty = true;
    for name in fs.entries(&trash).unwrap_or_default() {
        if let Err(why) = fs.remove_tree(&trash.join(&name)) {
            tracing::debug!(%why, "a file in the Git trees' trash is still in use");
            empty = false;
        }
    }
    empty
}
