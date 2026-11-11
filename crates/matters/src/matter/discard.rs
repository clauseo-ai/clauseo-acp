//! Undoing a create or import: on its own failure or cancel ([`undo_make`]), and when the host
//! could not record the matter it made ([`Matter::discard`]). Only what that create or import
//! made is removed: a `.clauseo` or a folder that was there before stays.

use std::io;
use std::path::Path;

use clauseo_protocol::MatterId;

use super::{Matter, exists, json, matter_home};
use crate::{Error, write};

/// What a create or import made besides the history, which it always writes.
#[derive(Debug, Clone, Copy)]
pub(super) struct Fresh {
    /// `matter.json` is this run's: false when an import adopted the one there.
    pub(super) marker: bool,
    /// `.clauseo` was not there before.
    pub(super) marker_dir: bool,
    /// The folder itself (a create).
    pub(super) folder: bool,
}

impl Fresh {
    /// A create: it makes the folder, so `.clauseo` and `matter.json` too.
    pub(super) const CREATED: Self = Self {
        marker: true,
        marker_dir: true,
        folder: true,
    };

    /// An import into `folder`, before it writes anything; `adopted` when it keeps the
    /// `matter.json` there, which names the matter already.
    pub(super) fn imported(folder: &Path, adopted: bool) -> Result<Self, Error> {
        Ok(Self {
            marker: !adopted,
            marker_dir: !exists(&folder.join(json::DIR))?,
            folder: false,
        })
    }

    /// Removes this run's `matter.json`, with `.clauseo` when it made that too; an adopted
    /// marker stays byte for byte.
    fn remove_marker(self, folder: &Path) -> Result<(), Error> {
        if self.marker {
            json::remove(folder, self.marker_dir)
        } else {
            Ok(())
        }
    }
}

/// [`Matter::discard`]: undoes this run's [`Matter::create`] or [`Matter::import`], for a host
/// that could not record the matter it made: removes the matter's history
/// (`<matters root>/<matter id>`) and `.clauseo/matter.json`, then `.clauseo` when the create or
/// import made it and it is empty, and the folder when `create` made it and it is empty. A file
/// put there since stays, and so does its folder; so does a `matter.json` the import adopted,
/// byte for byte. Afterwards the folder imports again as it did.
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
    let marker = fresh.remove_marker(&folder);
    if fresh.folder
        && let Err(err) = write::remove_dir(&folder)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::debug!(kind = ?err.kind(), stage = "folder", "the new folder was kept");
    }
    history.and(marker)
}

/// Removes what `Matter::make` wrote: the matter's folder under the matters root, and
/// `matter.json` when it wrote one (with `.clauseo` when `fresh` says the create or import made
/// it); an adopted `matter.json` stays as it was. Failures are logged; the caller's result
/// stands.
pub(super) fn undo_make(home: &Path, folder: &Path, fresh: Fresh) {
    if let Err(err) = write::remove_dir_all(home)
        && err.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(kind = ?err.kind(), stage = "history", "history folder left behind");
    }
    if let Err(err) = fresh.remove_marker(folder) {
        tracing::warn!(code = %err.code(), stage = "marker", "matter.json left behind");
    }
}

/// After a cancelled, failed or discarded create or import of matter `id`: removes
/// `folder`'s `.clauseo/matter.json` if it names `id` (a sync client put back the one that
/// create or import wrote and removed), and `.clauseo` if that leaves it empty. `Ok(true)` when
/// it removed it; a marker naming another matter, an unreadable one, or none, is left as it is.
/// Never reads more than the marker.
pub fn remove_marker_of(folder: &Path, id: MatterId) -> Result<bool, Error> {
    if json::read(folder)? != json::Found::Matter(id) {
        return Ok(false);
    }
    json::remove(folder, true)?;
    Ok(true)
}
