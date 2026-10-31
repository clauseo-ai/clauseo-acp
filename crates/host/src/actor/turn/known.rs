//! A checkpoint id the store does not know is recorded before use. R8 reuses its last
//! checkpoint when the folder equals it; if the commit that recorded that checkpoint never
//! landed (a crash, or a failed commit, between R8 and the log), the id would reach
//! `TurnStarted` or `TurnEnded` unrecorded. So a reused id is looked up in the matter's log, and
//! one it does not hold is recorded from R8's timeline in the same commit that uses it.

use clauseo_protocol::{
    CheckpointId, CheckpointRecorded, DomainEvent, MatterId, StreamId, ThreadId, ViewEntry,
};

use super::super::Actor;
use super::matter_of;
use crate::error::Error;
use crate::matter_worker::{Reused, TurnCheckpoint};

/// Runs `then` with a turn checkpoint's result once the matter's stream is loaded, when R8
/// reused an id, so [`unrecorded`] can read the matter's log; at once otherwise.
pub(super) fn when_loaded(
    actor: &mut Actor,
    thread: ThreadId,
    result: Result<TurnCheckpoint, Error>,
    then: impl FnOnce(&mut Actor, Result<TurnCheckpoint, Error>) + 'static,
) {
    let reused = result.as_ref().is_ok_and(|done| done.reused.is_some());
    let matter = matter_of(actor, thread);
    let stream = match matter {
        Ok(matter) if reused => StreamId::Matter(matter),
        Ok(_) | Err(_) => {
            then(actor, result);
            return;
        }
    };
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            if let Err(error) = loaded {
                tracing::warn!(%stream, %error, "a matter's log was not read; recording anyway");
            }
            then(actor, result);
            actor.release(stream);
        }),
    );
}

/// The `CheckpointRecorded` of the id R8 reused, with the kind, thread and turn of R8's
/// timeline, when the matter's log does not hold it and no commit in flight records it.
/// `None` for a new checkpoint (its own events record it) or a known one.
pub(super) fn unrecorded(
    actor: &Actor,
    matter: MatterId,
    done: &TurnCheckpoint,
) -> Result<Option<CheckpointRecorded>, Error> {
    let Some(Reused { entry, .. }) = &done.reused else {
        return Ok(None);
    };
    let id = done.checkpoint.outcome.id();
    if recorded(actor, matter, id) {
        return Ok(None);
    }
    let Some(entry) = entry else {
        tracing::error!(%id, "R8 reused a checkpoint its timeline does not hold");
        return Ok(None);
    };
    let recorded = CheckpointRecorded::new(id, entry.kind, entry.thread, entry.turn)
        .map_err(|error| Error::Internal(error.to_string()))?;
    Ok(Some(recorded))
}

/// Whether the matter's log holds checkpoint `id`, or a commit in flight records it. A matter
/// whose stream is not loaded (its read failed) is taken not to: recording an id twice only
/// makes the reducer note a duplicate, while using one unrecorded loses the version.
fn recorded(actor: &Actor, matter: MatterId, id: CheckpointId) -> bool {
    let stream = StreamId::Matter(matter);
    let durable = actor
        .loaded(stream)
        .and_then(|loaded| loaded.state.as_matter())
        .is_some_and(|state| {
            state.entries().values().rev().any(
                |entry| matches!(entry, ViewEntry::Version(version) if version.checkpoint == id),
            )
        });
    durable
        || actor.pipeline.unreleased(stream).any(|envelope| {
            matches!(&envelope.event, DomainEvent::CheckpointRecorded(recorded)
                if recorded.checkpoint() == id)
        })
}
