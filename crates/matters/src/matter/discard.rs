//! Undoing a create or import: on its own failure or cancel ([`undo_make`]), and when the host
//! could not record the matter it made ([`Matter::discard`]). Only what that create or import
//! made is removed: a `.clauseo` or a folder that was there before stays.

use std::io;
use std::path::Path;

use super::{Matter, exists, json, matter_home};
use crate::{Error, write};

/// What a create or import made besides the history and `matter.json`, which it always writes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Fresh {
    /// `.clauseo` was not there before.
    pub(super) marker_dir: bool,
    /// The folder itself (a create).
    pub(super) folder: bool,
}

impl Fresh {
    /// A create: it makes the folder, so `.clauseo` too.
    pub(super) const CREATED: Self = Self {
        marker_dir: true,
        folder: true,
    };

    /// An import into `folder`, before it writes anything.
    pub(super) fn imported(folder: &Path) -> Result<Self, Error> {
        Ok(Self {
            marker_dir: !exists(&folder.join(json::DIR))?,
            folder: false,
        })
    }
}

/// [`Matter::discard`]: undoes this run's [`Matter::create`] or [`Matter::import`], for a host
/// that could not record the matter it made: removes the matter's history
/// (`<matters root>/<matter id>`) and `.clauseo/matter.json`, then `.clauseo` when the create or
/// import made it and it is empty, and the folder when `create` made it and it is empty. A file
/// put there since stays, and so does its folder. Afterwards the folder imports as a new matter
/// again.
///
/// Refused with `Error::History` for a matter that was opened, not made by this run: its
/// history holds versions the lawyer has. The first removal that fails is returned; the
/// steps after it still run.
pub(super) fn discard(matter: Matter) -> Result<(), Error> {
    let Matter {
        id,
        folder,
        settings,
        core,
        fresh,
    } = matter;
    let Some(fresh) = fresh else {
        return Err(Error::history(
            "discard",
            "the matter was opened, not made by this run",
        ));
    };
    // The history's files close before they are removed (Windows).
    drop(core);
    let home = matter_home(&settings, id);
    let history = match write::remove_dir_all(&home) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(Error::io("remove", &home, err)),
        _ => Ok(()),
    };
    let marker = json::remove(&folder, fresh.marker_dir);
    if fresh.folder
        && let Err(err) = write::remove_dir(&folder)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::debug!(path = %folder.display(), error = %err, "the new folder was kept");
    }
    history.and(marker)
}

/// Removes what `Matter::make` wrote: the matter's folder under the matters root, and
/// `matter.json` (with `.clauseo` when `fresh` says the create or import made it). Failures
/// are logged; the caller's result stands.
pub(super) fn undo_make(home: &Path, folder: &Path, fresh: Fresh) {
    if let Err(err) = write::remove_dir_all(home)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(path = %home.display(), error = %err, "history folder left behind");
    }
    if let Err(err) = json::remove(folder, fresh.marker_dir) {
        tracing::warn!(error = %err, "matter.json left behind");
    }
}
