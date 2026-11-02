//! Who may take a matter's writer, as other parts of the host ask: whether it is busy for a
//! relocate or a forget, taking and giving it back for a relocate, and stopping a matter's agents.

use clauseo_protocol::{MatterId, ThreadId};

use super::super::Actor;
use super::{Holder, agents, end};

/// Whether `matter`'s writer is taken or soon wanted: a turn or an undo holds it, a turn waits
/// for it, or a thread of the matter has a turn, an undo not yet logged or a start-up repair in
/// progress (`matter.turn_running` for `RelocateMatter`).
pub(crate) fn writer_busy(actor: &Actor, matter: MatterId) -> bool {
    let held = actor
        .matters
        .get(&matter)
        .is_some_and(|entry| entry.turns.holder.is_some() || !entry.turns.queue.is_empty());
    held || actor
        .threads
        .values()
        .filter(|entry| entry.row.matter == matter)
        .any(|entry| {
            let state = &entry.turn;
            state.current.is_some()
                || state.closing.is_some()
                || state.undo.is_some()
                || state.reconciling
        })
}

/// Takes `matter`'s writer for a relocate when it is free (see [`writer_busy`]); `false` when
/// it is not.
pub(crate) fn hold_for_relocate(actor: &mut Actor, matter: MatterId) -> bool {
    if writer_busy(actor, matter) {
        return false;
    }
    match actor.matters.get_mut(&matter) {
        Some(entry) => {
            entry.turns.holder = Some(Holder::Relocate);
            true
        }
        None => false,
    }
}

/// The relocate holding `matter`'s writer is done: the next queued turn takes it.
pub(crate) fn release_relocate(actor: &mut Actor, matter: MatterId) {
    let held = actor
        .matters
        .get(&matter)
        .is_some_and(|entry| entry.turns.holder == Some(Holder::Relocate));
    if held {
        end::pass_writer(actor, matter);
    }
}

/// Whether anything runs in `matter` that `ForgetMatter` must wait for: a turn, undo, restore or
/// relocate holds its writer or turns wait for it, or one of its threads has a turn not yet
/// ended (one still launching takes the writer later) or an undo whose log is not durable.
pub(crate) fn matter_busy(actor: &Actor, matter: MatterId) -> bool {
    let writer = actor.matters.get(&matter).is_some_and(|entry| {
        entry.turns.holder.is_some()
            || !entry.turns.queue.is_empty()
            || entry.turns.restore.is_some()
    });
    writer
        || actor.threads.values().any(|entry| {
            entry.row.matter == matter
                && (entry.turn.current.is_some()
                    || entry.turn.closing.is_some()
                    || entry.turn.undo.is_some())
        })
}

/// Stops the agent of every thread of `matter` with the shutdown ladder (`ForgetMatter`); each
/// posts `Ended` and logs its detach as any stop does.
pub(crate) fn stop_agents(actor: &mut Actor, matter: MatterId) {
    let threads: Vec<ThreadId> = actor
        .threads
        .values()
        .filter(|entry| entry.row.matter == matter && entry.turn.agent.is_some())
        .map(|entry| entry.row.id)
        .collect();
    for thread in threads {
        agents::stop(actor, thread);
    }
}
