//! The worker's import: `Matter::import`, then the folder check, on the matter's thread.
//!
//! A folder whose marker names a matter the registry does not hold (Clauseo's data was wiped,
//! or the folder came from a colleague's install) is not refused. With no history of that
//! matter here, nothing is written and the import ends as [`ImportEnd::Adopt`]: the actor
//! imports it again under that id, on a worker of that id (`actor/matters/answer.rs`). With a
//! history of it here, which no row lists (a record that failed), adopting it would mix two
//! histories: the marker is set aside and the folder imported under the minted id.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use clauseo_matters::{Imported, Matter, NewMatter, Refusal, Settings};

use super::{Claimed, ImportEnd, Slot, root_commit};
use crate::agent::folder_support::CheckSource;
use crate::error::Error;

pub(super) fn import(
    settings: &Settings,
    slot: &mut Slot,
    new: NewMatter,
    (refused, registry): (&[PathBuf], &[Claimed]),
    cancel: &AtomicBool,
    check: CheckSource,
) -> Result<ImportEnd, Error> {
    let (root, id, folder) = (new.checkpoint, new.id, new.folder.clone());
    let at = new.created_at;
    let imported = match Matter::import(settings.clone(), new.clone(), refused, &mut |_| {}, cancel)
    {
        Err(clauseo_matters::Error::FolderRefused {
            reason: Refusal::AlreadyMatter(old),
            ..
        }) if !registry.iter().any(|claimed| claimed.matter == old) => {
            if !clauseo_matters::history_exists(settings, old)? {
                return Ok(ImportEnd::Adopt(old));
            }
            clauseo_matters::set_aside_marker(&folder, at)?;
            tracing::warn!(
                matter = %id,
                %old,
                "a marker naming a matter with a history but no row was set aside"
            );
            Matter::import(settings.clone(), new, refused, &mut |_| {}, cancel)?
        }
        other => other?,
    };
    match imported {
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
