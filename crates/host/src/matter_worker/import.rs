//! The worker's import: `Matter::import`, then the folder check, on the matter's thread; how it
//! ended ([`ImportEnd`]), and what a create or import that made a matter hands the actor
//! ([`made`]).
//!
//! A folder whose marker names a matter the registry does not hold (Clauseo's data was wiped,
//! or the folder came from a colleague's install) is not refused. With no history of that
//! matter here, nothing is written and the import ends as [`ImportEnd::Adopt`]: the actor
//! imports it again under that id, on a worker of that id (`actor/matters/answer.rs`). With a
//! history of it here, which no row lists (a record that failed), adopting it would mix two
//! histories: the marker is set aside and the folder imported under the minted id.
//!
//! A marker naming a matter the registry does hold (listed, forgotten or being made) in another
//! folder is a copy or a move ([`recorded`]). A copy (that matter's folder is still there, or
//! whether it is cannot be told) becomes a new matter: its marker is set aside and the folder
//! imported under the minted id, so the matter it was copied from is never taken over. A move
//! (that folder is gone) ends refused as already that matter, and the actor relinks a listed one
//! to this folder or brings a forgotten one back here (`answer.rs`).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use clauseo_matters::{CommitId, ImportWarning, Imported, Matter, NewMatter, Refusal, Settings};
use clauseo_protocol::{CommandId, FolderSupportKind, ImportSummary, MatterId};

use super::{Claimed, MatterReply, Slot, is_same_folder, root_commit};
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
        Err(
            error @ clauseo_matters::Error::FolderRefused {
                reason: Refusal::AlreadyMatter(old),
                ..
            },
        ) => {
            match recorded(old, &folder, registry) {
                Recorded::Unknown if !clauseo_matters::history_exists(settings, old)? => {
                    return Ok(ImportEnd::Adopt(old));
                }
                Recorded::Unknown => tracing::warn!(
                    matter = %id,
                    %old,
                    "a marker naming a matter with a history but no row is set aside"
                ),
                Recorded::Copied => tracing::info!(
                    matter = %id,
                    %old,
                    "a copy of a known matter's folder is imported as a new matter"
                ),
                Recorded::Here | Recorded::Moved => return Err(Error::Matters(error)),
            }
            clauseo_matters::set_aside_marker(&folder, at)?;
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

/// Where the matter `old` that `folder`'s marker names is recorded.
enum Recorded {
    /// Nowhere in the registry.
    Unknown,
    /// At `folder` itself, by any spelling.
    Here,
    /// Elsewhere, and every folder recorded for it is gone (not found).
    Moved,
    /// Elsewhere, and a folder recorded for it is still there, or whether it is cannot be told
    /// (a read error other than not found): never taken over on doubt.
    Copied,
}

fn recorded(old: MatterId, folder: &Path, registry: &[Claimed]) -> Recorded {
    let folders: Vec<&Path> = registry
        .iter()
        .filter(|claimed| claimed.matter == old)
        .map(|claimed| claimed.folder.as_path())
        .collect();
    if folders.is_empty() {
        Recorded::Unknown
    } else if folders.iter().any(|own| is_same_folder(folder, own)) {
        Recorded::Here
    } else if folders
        .iter()
        .all(|own| std::fs::metadata(own).is_err_and(|err| err.kind() == io::ErrorKind::NotFound))
    {
        Recorded::Moved
    } else {
        Recorded::Copied
    }
}

/// How an import ended.
#[derive(Debug)]
pub(crate) enum ImportEnd {
    /// The matter exists; the worker keeps it.
    Done {
        /// The root checkpoint's commit.
        commit: CommitId,
        summary: ImportSummary,
        warnings: Vec<ImportWarning>,
        /// The folder check's answer (`agent/folder_support.rs`), made on this thread.
        folder_support: Option<FolderSupportKind>,
    },
    /// The cancel flag was set; nothing was kept.
    Cancelled,
    /// The folder is this listed matter's own folder; nothing was made.
    Listed(MatterId),
    /// The folder is the one this create or import in flight is making; nothing was made.
    Making(CommandId),
    /// The folder's marker names this matter, which the registry does not hold and which has no
    /// history here: nothing was written, and the host imports the folder again under its id.
    Adopt(MatterId),
}

/// A made matter's root commit and import summary, and its folder check's answer.
pub(crate) type Made = ((CommitId, Option<ImportSummary>), Option<FolderSupportKind>);

/// The root checkpoint's commit and the import summary of a create or import that succeeded
/// (`None` for a new folder), and the folder check's answer.
pub(crate) fn made(reply: MatterReply) -> Result<Made, Error> {
    match reply {
        MatterReply::Created(result) => result.map(|(commit, support)| ((commit, None), support)),
        MatterReply::Imported(result) => match result? {
            ImportEnd::Done {
                commit,
                summary,
                warnings,
                folder_support,
            } => {
                for warning in &warnings {
                    tracing::info!(?warning, "import warning");
                }
                Ok(((commit, Some(summary)), folder_support))
            }
            ImportEnd::Cancelled => Err(Error::Closed),
            // Answered by the actor before it gets here (`matters/answer.rs`).
            ImportEnd::Listed(_) | ImportEnd::Making(_) | ImportEnd::Adopt(_) => Err(
                Error::Internal("an import's folder was held by another matter".to_owned()),
            ),
        },
        MatterReply::Opened(_)
        | MatterReply::Located(_)
        | MatterReply::BeforeTurn(_)
        | MatterReply::AfterTurn(_)
        | MatterReply::Undone(_)
        | MatterReply::Restored(_)
        | MatterReply::Compared(_)
        | MatterReply::Previewed(_)
        | MatterReply::EditedSince(_)
        | MatterReply::HeldOpen(_)
        | MatterReply::Papers(_)
        | MatterReply::StillGone(_)
        | MatterReply::SidesWritten(_)
        | MatterReply::Revisions(_)
        | MatterReply::Discarded(_) => Err(Error::Internal(
            "a matter's create got another job's reply".to_owned(),
        )),
    }
}
