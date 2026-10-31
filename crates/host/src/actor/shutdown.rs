//! Shutdown: stop taking commands, let the turn path cancel running turns and stop agents,
//! wait for every commit and snapshot in flight, then stop the snapshot and matter threads and
//! close the store.

use clauseo_protocol::HostError;

use super::input::Reply;
use super::{Actor, Phase, turn};
use crate::error::Error;

/// Starts shutdown, or adds `reply` to a shutdown already running. The loop ends once
/// the work in flight is done (see `Actor::settle`).
pub(super) fn begin_shutdown(actor: &mut Actor, reply: Option<Reply<()>>) {
    if let Phase::Stopping(replies) = &mut actor.phase {
        replies.extend(reply);
        return;
    }
    actor.phase = Phase::Stopping(reply.into_iter().collect());
    super::matters::cancel_imports(actor);
    turn::shutdown(actor);
}

/// Stops the snapshot thread (its queue is already empty), the matter threads, and closes the
/// store.
pub(super) async fn close(actor: Actor) -> Result<(), Error> {
    let Actor {
        store,
        snapshots,
        matters,
        feeder,
        ..
    } = actor;
    if let Err(error) = snapshots.stop() {
        tracing::warn!(%error, "the snapshot thread ended badly");
    }
    for (_, matter) in matters {
        if let Some(worker) = matter.worker
            && let Err(error) = worker.stop()
        {
            tracing::warn!(%error, "a matter thread ended badly");
        }
    }
    drop(feeder);
    store.close().await.map_err(Error::Store)
}

/// Answers every shutdown call: `Ok`, or the close's error.
pub(super) fn answer(closed: Result<(), Error>, replies: Vec<Reply<()>>) -> Result<(), Error> {
    let shared = closed
        .as_ref()
        .copied()
        .map_err(|error| HostError::new(error.code(), error.to_string()));
    for reply in replies {
        let _gone = reply.send(shared.clone().map_err(Error::Refused));
    }
    closed
}
