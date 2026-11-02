//! What a matter's thread needs for "Locate folder" and the relink of a deleted `matter.json`:
//! the registry a folder is checked against, the relink an open may use, and the two calls.

use std::path::{Path, PathBuf};

use clauseo_matters::{Matter, Settings};
use clauseo_protocol::{CheckpointId, ErrorCode, MatterId, Timestamp};

use super::Slot;
use crate::error::Error;

/// What a folder is checked against before it becomes a matter's folder again (locate) or gets
/// its `matter.json` back (relink): the folders import refuses, and every other registered
/// matter's folder. Built on the actor from memory; the paths are compared here, off the actor,
/// since the comparison canonicalises them.
#[derive(Debug, Clone)]
pub(crate) struct Registry {
    /// The protected folders import refuses (`HostConfig::with_refused_folders` and the data
    /// folder).
    pub(crate) refused: Vec<PathBuf>,
    /// Every other registered matter and its folder.
    pub(crate) others: Vec<(MatterId, PathBuf)>,
}

impl Registry {
    /// `matter.folder_refused` when `folder` is, holds or sits inside another matter's folder.
    pub(super) fn check(&self, folder: &Path) -> Result<(), Error> {
        match self
            .others
            .iter()
            .find(|(_, other)| clauseo_matters::same_or_nested(folder, other))
        {
            Some((other, _)) => Err(Error::refused(
                ErrorCode::MatterFolderRefused,
                format!(
                    "the folder may not be this matter's: it is, holds or sits inside the \
                     folder of matter {other}"
                ),
            )),
            None => Ok(()),
        }
    }
}

/// What an open needs to put back a deleted or unreadable `matter.json`, once.
#[derive(Debug, Clone)]
pub(crate) struct Relink {
    /// The matter's creation time, from the host's registry.
    pub(crate) created_at: Timestamp,
    pub(crate) registry: Registry,
}

/// "Locate folder": refuses a folder another matter claims, then drops the matter this worker
/// holds (R8 opens one `Matter` per history) and locates it in `folder`. A refusal leaves the
/// matter to be opened again from `current`.
pub(super) fn locate(
    settings: &Settings,
    slot: &mut Slot,
    id: MatterId,
    folder: &Path,
    current: PathBuf,
    registry: &Registry,
) -> Result<Option<CheckpointId>, Error> {
    registry.check(folder)?;
    *slot = Slot::Empty;
    match Matter::locate(settings.clone(), id, folder, &registry.refused) {
        Ok(matter) => {
            let last = matter.last_checkpoint();
            *slot = Slot::Open(Box::new(matter));
            Ok(last)
        }
        Err(error) => {
            *slot = Slot::Unopened {
                id,
                folder: current,
                relink: None,
            };
            Err(error.into())
        }
    }
}

/// `Matter::relink` of matter `id` in `folder`, unless another registered matter claims the
/// folder; then `missing`, the open's own refusal, stands.
pub(super) fn relink_matter(
    settings: &Settings,
    id: MatterId,
    folder: &Path,
    relink: &Relink,
    missing: clauseo_matters::Error,
) -> Result<Matter, Error> {
    if let Err(claimed) = relink.registry.check(folder) {
        tracing::warn!(%id, error = %claimed, "matter.json is gone; not written again");
        return Err(missing.into());
    }
    let matter = Matter::relink(
        settings.clone(),
        id,
        folder,
        relink.created_at,
        &relink.registry.refused,
    )?;
    tracing::warn!(%id, folder = %folder.display(), "matter.json was gone; written again");
    Ok(matter)
}
