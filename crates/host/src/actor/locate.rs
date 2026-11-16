//! What the desktop's `open_file` and `reveal` ask the host
//! (`docs/decisions/0026-open-and-reveal.md`): the file a ref names, checked at call time against
//! the matter that owns it, and a matter's folder.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{ErrorCode, FileRefId, MatterId};

use super::input::{Input, Reply};
use super::matters::probe::{NO_ANSWER, PROBE_WAIT};
use super::{Actor, HostPhase};
use crate::LocatedFile;
use crate::agent::codex::prepare::{Job, off_actor};
use crate::error::Error;
use crate::log_file::Cause;

/// The file `file` names, with the matter that owns it and that matter's folder, answered on
/// `reply`. The owner is the matter whose recorded folder the path is written under (the
/// deepest, were folders nested), found on the actor; the check that the folder is there and
/// that the file lies in it runs now, on the paths as they resolve (`file_link::inside`), since
/// the folder or the file may have moved since the ref was minted. That check may hang on a
/// folder on a share that does not answer, so it runs off the actor (`matters/probe.rs`),
/// waited on for [`PROBE_WAIT`], and its answer comes back as [`Input::Located`].
/// `command.invalid` for a ref this run did not issue and for a file in no matter's folder or
/// outside its owner's; `matter.folder_missing` when the owner's folder is gone or did not
/// answer in time.
pub(super) fn locate_file(actor: &mut Actor, file: FileRefId, reply: Reply<LocatedFile>) {
    let owner = owner(actor, file);
    let (matter, folder, path) = match owner {
        Ok(owner) => owner,
        Err(error) => {
            let _gone = reply.send(Err(error));
            return;
        }
    };
    let job = Job {
        what: "file locate check",
        matter: Some(matter),
        wait: PROBE_WAIT,
    };
    super::matters::probe::begun(actor);
    let (clock, probe) = (
        Arc::clone(&actor.clock),
        Arc::clone(&actor.folder_probes.locate),
    );
    actor.spawn_post(async move {
        let (asked_path, asked_folder) = (path.clone(), folder.clone());
        let found = off_actor(clock, job, move || Ok(probe(&asked_path, &asked_folder))).await;
        Input::Located(Box::new(Located {
            file,
            target: LocatedFile {
                matter,
                folder,
                path,
            },
            found,
            reply,
        }))
    });
}

/// The matter that owns the file `file` names, its folder and the file's path.
fn owner(actor: &Actor, file: FileRefId) -> Result<(MatterId, PathBuf, PathBuf), Error> {
    let path = actor.files.get(&file).ok_or_else(|| {
        Error::refused(
            ErrorCode::CommandInvalid,
            format!("file ref {file} was not issued in this run"),
        )
    })?;
    let (matter, folder) = actor
        .matters
        .iter()
        .map(|(id, entry)| (*id, PathBuf::from(&entry.row.folder_path)))
        .filter(|(_, folder)| path.starts_with(folder))
        .max_by_key(|(_, folder)| folder.components().count())
        .ok_or_else(|| {
            Error::refused(
                ErrorCode::CommandInvalid,
                format!("file ref {file} names a file in no matter's folder"),
            )
        })?;
    Ok((matter, folder, path.clone()))
}

/// A `LocateFile`'s probe answer, with what it answers.
pub(crate) struct Located {
    file: FileRefId,
    /// The answer, should the probe pass.
    target: LocatedFile,
    /// Whether the folder is there and the file lies in it, or `off_actor`'s failure.
    found: Result<(bool, bool), Error>,
    reply: Reply<LocatedFile>,
}

/// The probe answered: the call is answered once the host is not stopping and the matter is
/// still listed with the folder probed. A late answer, with the actor gone, is dropped with
/// its reply (the caller hears the host closed).
pub(super) fn on_located(actor: &mut Actor, located: Located) {
    super::matters::probe::answered(actor);
    let Located {
        file,
        target,
        found,
        reply,
    } = located;
    let matter = target.matter;
    let owner_now = owner(actor, file).map(|(owner, folder, _)| (owner, folder));
    let answer = if matches!(actor.phase, HostPhase::Stopping(_)) {
        Err(Error::Closed)
    } else if let Err(error) = owner_now {
        // Forgotten while its folder was checked (or its ref gone): as if asked now.
        Err(error)
    } else if owner_now.as_ref().ok() != Some(&(matter, target.folder.clone())) {
        Err(if actor.matters.contains_key(&matter) {
            // Still listed, relocated while its folder was checked: the folder probed is not
            // its own any more.
            Error::refused(
                ErrorCode::MatterFolderMissing,
                format!("the folder of matter {matter} changed while it was checked"),
            )
        } else {
            Error::refused(
                ErrorCode::CommandInvalid,
                format!("the matter that owns file ref {file} changed while it was checked"),
            )
        })
    } else {
        match found {
            Ok((true, true)) => Ok(target),
            Ok((false, _)) => Err(Error::refused(
                ErrorCode::MatterFolderMissing,
                format!("the folder of matter {matter} is not there"),
            )),
            Ok((true, false)) => Err(Error::refused(
                ErrorCode::CommandInvalid,
                format!("file ref {file} names a file outside the folder of matter {matter}"),
            )),
            Err(error) => {
                tracing::warn!(error = %Cause(&error), matter = %matter, "a located file's folder did not answer");
                Err(Error::refused(ErrorCode::MatterFolderMissing, NO_ANSWER))
            }
        }
    };
    let _gone = reply.send(answer);
}

/// The folder recorded for `matter`: `command.unknown_matter` for a matter the host does not
/// know. Whether the folder is there is the caller's to check.
pub(super) fn matter_folder(actor: &Actor, matter: MatterId) -> Result<PathBuf, Error> {
    actor
        .check_matter(matter)
        .map(|entry| PathBuf::from(&entry.row.folder_path))
}
