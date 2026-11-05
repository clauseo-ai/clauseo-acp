//! Shutdown: stop taking commands, let the turn path cancel running turns and stop agents,
//! wait for every commit, snapshot and run folder removal in flight, then stop the snapshot
//! and matter threads and close the store.

use super::input::Reply;
use super::{Actor, HostPhase, turn};
use crate::error::Error;
use crate::log_file::Cause;
use crate::log_file::LIFECYCLE;

/// Starts shutdown, or adds `reply` to a shutdown already running. The loop ends once
/// the work in flight is done (see `Actor::settle`).
pub(super) fn begin_shutdown(actor: &mut Actor, reply: Option<Reply<()>>) {
    if let HostPhase::Stopping(replies) = &mut actor.phase {
        replies.extend(reply);
        return;
    }
    actor.phase = HostPhase::Stopping(reply.into_iter().collect());
    super::matters::cancel_imports(actor);
    super::scan::cancel_all(actor);
    super::browser_install::shutdown(actor);
    turn::shutdown(actor);
}

/// Stops the snapshot thread (its queue is already empty), the matter threads, and closes the
/// store.
pub(super) async fn close(actor: Actor) -> Result<(), Error> {
    let Actor {
        store,
        snapshots,
        search,
        matters,
        forgotten,
        feeder,
        ..
    } = actor;
    // Before the store closes: the search thread reads it.
    search.stop();
    if let Err(error) = snapshots.stop() {
        tracing::warn!(error = %Cause(&error), "the snapshot thread ended badly");
    }
    for (id, matter) in matters.into_iter().chain(forgotten) {
        let Some(worker) = matter.worker else {
            continue;
        };
        tracing::info!(target: LIFECYCLE, matter = %id, "matter closed");
        if let Err(error) = worker.stop() {
            tracing::warn!(error = %Cause(&error), "a matter thread ended badly");
        }
    }
    drop(feeder);
    let closed = store.close().await.map_err(Error::Store);
    tracing::info!(target: LIFECYCLE, ok = closed.is_ok(), "host stopped");
    closed
}

/// Answers every shutdown call: `Ok`, or the close's error.
pub(super) fn answer(closed: Result<(), Error>, replies: Vec<Reply<()>>) -> Result<(), Error> {
    let shared = closed.as_ref().copied().map_err(Error::to_wire);
    for reply in replies {
        let _gone = reply.send(shared.clone().map_err(Error::Refused));
    }
    closed
}
