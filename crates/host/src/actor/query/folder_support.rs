//! `FolderSupport`: the folder check (`agent/folder_support.rs`) of the place a `CreateMatter`
//! with the same `MatterSource` would use, before the matter exists, for the New matter dialog.
//! The place and create's own refusals before it makes anything come from
//! `actor/matters/picked.rs`: a folder create refuses is refused here with the same code, and
//! never reaches the check. The check comes from the actor's [`CheckSource`], as a launch's
//! does. Both run off the actor ([`off_actor`], waited on for [`PREPARE_WAIT`]) and the answer
//! goes straight to the caller from a task of its own: nothing is kept, as there is no matter's
//! row. A check that doesn't answer in time (or whose thread could not start or ended without an
//! answer) answers `check_failed`, as a failed check does: an unknown folder, never an error.
//! The log names no folder.
//!
//! [`CheckSource`]: crate::agent::folder_support::CheckSource
//! [`PREPARE_WAIT`]: crate::agent::codex::prepare::PREPARE_WAIT

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_protocol::{FolderSupportKind, MatterSource, PickedFolderSupport, QueryResult};
use futures::task::LocalSpawnExt;

use crate::actor::Actor;
use crate::actor::input::Reply;
use crate::agent::codex::prepare::{Job, off_actor};
use crate::agent::folder_support::{Answer, FolderCheck};
use crate::clock::Clock;
use crate::error::Error;

/// Answers `FolderSupport` for `source`: `command.invalid` at once for a ref the host never
/// gave, else create's refusal or the check's answer, from a task of its own.
pub(super) fn folder_support(actor: &Actor, source: &MatterSource, reply: Reply<QueryResult>) {
    let place = match crate::actor::matters::picked::place(actor, source) {
        Ok(place) => place,
        Err(error) => {
            let _gone = reply.send(Err(error));
            return;
        }
    };
    let check = FolderCheck {
        matter: None,
        source: actor.folder_check,
    };
    let clock = Arc::clone(&actor.clock);
    let spawned = actor.spawner.spawn_local(async move {
        let answer = picked(clock, place, move |path| check.run(&path)).await;
        let answer =
            answer.map(|support| QueryResult::FolderSupport(PickedFolderSupport { support }));
        let _gone = reply.send(answer);
    });
    if let Err(error) = spawned {
        tracing::error!(%error, "the host pool refused a folder check");
    }
}

/// The row's value for the place `place` gives, checked by `check`, both off the actor and
/// waited on for [`Job::short`]'s wait on `clock`. `place`'s refusal is the answer's error;
/// past the wait, or when the thread could not start or ended without an answer, the answer is
/// `check_failed`.
pub(super) async fn picked(
    clock: Arc<dyn Clock>,
    place: impl FnOnce() -> Result<PathBuf, Error> + Send + 'static,
    check: impl FnOnce(PathBuf) -> Answer + Send + 'static,
) -> Result<Option<FolderSupportKind>, Error> {
    let job = Job::short("picked folder check", None);
    // The outer `Err` is `off_actor`'s own; the inner one is create's refusal.
    match off_actor(clock, job, move || Ok(place().map(check))).await {
        Ok(checked) => checked.map(Answer::row_kind),
        Err(_unanswered) => Ok(Answer::Failed.row_kind()),
    }
}

#[cfg(test)]
mod tests;
