//! What the updater's install asks of the turn path (decision record 0037, `actor/update.rs`):
//! whether any turn runs, Stop for every running turn, and the queues moved on once the hold
//! on new sends is lifted. The hold itself is in `hold.rs`.

use clauseo_protocol::ThreadId;

use super::super::super::Actor;
use super::super::{TurnState, cancel};
use super::{move_on, moves_by_itself};

/// Whether any thread has a turn running, for `after_turn`: a current turn (queued for the
/// writer, starting or prompting), a last turn whose `TurnEnded` is on its way to the store,
/// or queued turns that start by themselves once the current one ends. A held queue does not
/// count: it waits for the lawyer (after a stop other than `EndTurn`, and at every start), so
/// it would keep an install waiting for as long as the lawyer leaves it. Nor does a turn whose
/// end failed twice: it ends only at the next start, and its thread takes no send meanwhile.
/// That is `turn_running`'s rule for `ArchiveThread`, those two apart.
pub(crate) fn any_running(actor: &Actor) -> bool {
    actor.threads.values().any(|entry| {
        let state = &entry.turn;
        state.current.is_some() || closing(state) || moves_by_itself(&state.queue)
    })
}

/// Whether any thread still has a turn that has not ended: a current turn, or one whose
/// `TurnEnded` commit has not returned and is tried again in this run. Once none has, every
/// turn [`cancel_all`] cancelled has its `TurnEnded`, or is left for the next start.
pub(crate) fn any_active(actor: &Actor) -> bool {
    actor
        .threads
        .values()
        .any(|entry| entry.turn.current.is_some() || closing(&entry.turn))
}

/// A last turn whose `TurnEnded` this run still commits: not one whose end failed twice, which
/// the next start ends.
fn closing(state: &TurnState) -> bool {
    state
        .closing
        .as_ref()
        .is_some_and(|closing| !closing.retried)
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
        move_on(actor, thread);
    }
}
