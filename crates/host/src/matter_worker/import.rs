//! The worker's import: `Matter::import`, then the folder check, on the matter's thread.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use clauseo_matters::{Imported, Matter, NewMatter, Settings};

use super::{ImportEnd, Slot, root_commit};
use crate::agent::folder_support::CheckSource;
use crate::error::Error;

pub(super) fn import(
    settings: &Settings,
    slot: &mut Slot,
    new: NewMatter,
    refused: &[PathBuf],
    cancel: &AtomicBool,
    check: CheckSource,
) -> Result<ImportEnd, Error> {
    let (root, id, folder) = (new.checkpoint, new.id, new.folder.clone());
    match Matter::import(settings.clone(), new, refused, &mut |_progress| {}, cancel)? {
        Imported::Done {
            matter,
            summary,
            warnings,
        } => {
            let commit = root_commit(&matter, root)?;
            let folder_check = crate::agent::folder_support::FolderCheck {
                matter: Some(id),
                source: check,
            };
            let folder_support = folder_check.run(&folder).row_kind();
            *slot = Slot::Open(matter);
            Ok(ImportEnd::Done {
                commit,
                summary,
                warnings,
                folder_support,
            })
        }
        Imported::Cancelled => Ok(ImportEnd::Cancelled),
    }
}
