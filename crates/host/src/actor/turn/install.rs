//! What the updater's install asks of the turn path (decision record 0037, `actor/update.rs`):
//! whether any turn runs, Stop for every running turn, and the queues moved on once the hold
//! on new sends is lifted. The hold itself is in `queue.rs` (`route` and `move_on`).

use clauseo_protocol::ThreadId;

use super::super::Actor;
use super::{cancel, queue};

/// Whether any thread has a turn running, for `after_turn`: a current turn (queued for the
/// writer, starting or prompting), a last turn whose `TurnEnded` is not durable yet, or queued
/// turns that start by themselves once the current one ends. A held queue does not count: it
/// waits for the lawyer (after a stop other than `EndTurn`, and at every start), so it would
/// keep an install waiting for as long as the lawyer leaves it. That is `turn_running`'s rule
/// for `ArchiveThread`, held queues apart.
pub(crate) fn any_running(actor: &Actor) -> bool {
    actor.threads.values().any(|entry| {
        let state = &entry.turn;
        state.current.is_some() || state.closing.is_some() || queue::moves_by_itself(&state.queue)
    })
}

/// Whether any thread still has a turn that has not ended durably: a current turn, or one
/// whose `TurnEnded` commit has not returned. Once none has, every turn [`cancel_all`]
/// cancelled has its `TurnEnded`.
pub(crate) fn any_active(actor: &Actor) -> bool {
    actor
        .threads
        .values()
        .any(|entry| entry.turn.current.is_some() || entry.turn.closing.is_some())
}

/// `InstallUpdate { now }`: every current turn goes through Stop's path without a command
/// (`cancel::cancel_current`, as a send now does), so a prompting turn's descendants are swept
/// when it ends and a turn that has not started ends `Cancelled`. Queued turns stay queued.
pub(crate) fn cancel_all(actor: &mut Actor) {
    let threads: Vec<ThreadId> = actor
        .threads
        .iter()
        .filter(|(_, entry)| entry.turn.current.is_some())
        .map(|(thread, _)| *thread)
        .collect();
    for thread in threads {
        cancel::cancel_current(actor, thread);
    }
}

/// The hold on new sends is lifted: each thread's queue moves on as it would have, so a send
/// held while it lasted starts unless its queue is held for the lawyer.
pub(crate) fn release(actor: &mut Actor) {
    let threads: Vec<ThreadId> = actor.threads.keys().copied().collect();
    for thread in threads {
        queue::move_on(actor, thread);
    }
}
