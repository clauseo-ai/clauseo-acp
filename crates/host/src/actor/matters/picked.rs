//! The `FolderSupport` query's place (`actor/query/folder_support.rs`): the folder a
//! `CreateMatter` with the same source would make its matter in or under, and the refusals that
//! create makes before its matter is made, with the same codes, so the dialog never shows a
//! kind for a folder create refuses. Built on the actor (the refused list, the registry and the
//! names of the refusal codes); run off it, since the checks may touch the disk.
//!
//! - `ExistingFolder`: the import's own checks before it makes anything, in its order: the
//!   place check (`clauseo_matters::check_place`: a drive root, the home folder, a system
//!   folder, the data folder) and the registry (`check_registry`: a folder that is, holds or
//!   sits inside another matter's), as `MatterJob::Import` runs them on its worker, then the
//!   protected-tree check `Matter::import` runs (`clauseo_matters::check_protected`: inside
//!   the data folder or the matters root).
//! - `NewFolder`: the parent, canonicalised as `matter_folder` does. Create checks nothing else
//!   of a parent: the "exists" check and the registry check both need the new folder's name.
//! - `DefaultLocation`: the default location, as a new folder's parent. It may not exist before
//!   the first matter (create makes it), so its nearest folder that exists is checked: the one
//!   the new folders are made in.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterSource;

use super::{Actor, RefusalNames, check_registry, claimed_folders, refused_folders, registered};
use crate::error::Error;
use crate::matter_worker::matter_settings;

/// The refusals of [`place`], run off the actor: the folder to check, or create's refusal.
pub(crate) type PlaceCheck = Box<dyn FnOnce() -> Result<PathBuf, Error> + Send>;

/// The place `CreateMatter` with `source` would use, or `command.invalid` at once for a ref the
/// host never gave.
pub(crate) fn place(actor: &Actor, source: &MatterSource) -> Result<PlaceCheck, Error> {
    match *source {
        MatterSource::ExistingFolder { folder } => {
            let path = registered(actor, folder)?;
            let refused = refused_folders(actor);
            let registry = claimed_folders(actor);
            let names = RefusalNames::of(actor);
            let settings = matter_settings(actor.effective.data_dir());
            Ok(Box::new(move || {
                clauseo_matters::check_place(&path, &refused)
                    .map_err(Error::Matters)
                    .and_then(|()| check_registry(&path, &registry, true))
                    .and_then(|()| {
                        clauseo_matters::check_protected(&path, &settings).map_err(Error::Matters)
                    })
                    // The place check never answers `AlreadyMatter` (only an import's marker
                    // read does), so no listed matter is named here.
                    .map_err(|error| names.refusal(error, &path, |_| false))?;
                Ok(path)
            }))
        }
        MatterSource::NewFolder { folder } => {
            let parent = registered(actor, folder)?;
            Ok(Box::new(move || {
                Ok(dunce::canonicalize(&parent).unwrap_or(parent))
            }))
        }
        MatterSource::DefaultLocation => {
            let location = actor.effective.default_matters_dir().to_path_buf();
            Ok(Box::new(move || Ok(nearest_existing(&location))))
        }
    }
}

/// `folder`, or its nearest ancestor that exists, canonicalised (as written when that fails).
fn nearest_existing(folder: &Path) -> PathBuf {
    let mut at = folder;
    while !at.exists() {
        match at.parent() {
            Some(up) => at = up,
            None => break,
        }
    }
    dunce::canonicalize(at).unwrap_or_else(|_| at.to_path_buf())
}
