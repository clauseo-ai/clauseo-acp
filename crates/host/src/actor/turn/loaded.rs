//! What a thread's or a matter's turn state says to the rest of the host: whether a turn runs,
//! which one, and whether its stream stays loaded.

use clauseo_protocol::{MatterId, ThreadId, TurnId};

use super::super::Actor;

/// Whether `thread` has a current turn, a last turn whose end is not durable yet, or queued
/// turns (`thread.turn_running` for `ArchiveThread`).
pub(crate) fn turn_running(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads.get(&thread).is_some_and(|entry| {
        let state = &entry.turn;
        state.current.is_some() || state.closing.is_some() || !state.queue.is_empty()
    })
}

/// The turn running on `thread` now, if any (`blocked.rs` attributes a blocked site to it).
pub(crate) fn running_turn(actor: &Actor, thread: ThreadId) -> Option<TurnId> {
    let current = actor.threads.get(&thread)?.turn.current.as_deref()?;
    Some(current.turn)
}

/// Whether `thread`'s stream stays loaded: an agent, open turn, repair, retriable end, limit stop.
pub(crate) fn keeps_loaded(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads.get(&thread).is_some_and(|entry| {
        let state = &entry.turn;
        state.agent.is_some()
            || state.preparing.is_some()
            || state.current.is_some()
            || state.reconciling
            || state
                .closing
                .as_ref()
                .is_some_and(|closing| !closing.retried)
    }) || super::super::limits::holds_stop(actor, thread)
}

/// Whether `matter`'s stream, once loaded to check a reused checkpoint, stays loaded: while a
/// turn, an undo or a restore holds its writer, so the turn's next checkpoint reads it without
/// a reload.
pub(crate) fn keeps_matter_loaded(actor: &Actor, matter: MatterId) -> bool {
    actor
        .matters
        .get(&matter)
        .is_some_and(|entry| entry.turns.holder.is_some())
}
