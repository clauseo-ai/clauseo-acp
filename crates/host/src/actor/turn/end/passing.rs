//! The matter's writer passing on (step 8), split from `end.rs` for its size.

use clauseo_protocol::{MatterId, StreamId, ThreadId};

use super::super::super::Actor;
use super::super::{Holder, TurnPhase, current, host_stopping, steps};

/// `thread` lets go of `matter`'s writer (or its place in the queue); the next queued turn
/// takes it at step 4. Nothing new starts during shutdown.
pub(crate) fn release_writer(actor: &mut Actor, matter: MatterId, thread: ThreadId) {
    let Some(entry) = actor.matters.get_mut(&matter) else {
        return;
    };
    let turns = &mut entry.turns;
    if turns.holder != Some(Holder::Thread(thread)) {
        turns.queue.retain(|queued| *queued != thread);
        return;
    }
    pass_writer(actor, matter);
}

/// The holder of `matter`'s writer has let go: the next queued turn takes it, or nothing
/// does during shutdown. A file action it held is over (`last_exit.rs`).
pub(crate) fn pass_writer(actor: &mut Actor, matter: MatterId) {
    super::super::super::last_exit::finished(actor, matter);
    let shutting = host_stopping(actor);
    let Some(entry) = actor.matters.get_mut(&matter) else {
        return;
    };
    entry.turns.holder = None;
    if shutting {
        actor.release(StreamId::Matter(matter));
        return;
    }
    loop {
        let Some(next) = actor
            .matters
            .get_mut(&matter)
            .and_then(|entry| entry.turns.queue.pop_front())
        else {
            actor.release(StreamId::Matter(matter));
            return;
        };
        let waiting = current(actor, next)
            .filter(|current| current.phase == TurnPhase::Queued)
            .map(|current| current.turn);
        if let Some(turn) = waiting {
            if let Some(entry) = actor.matters.get_mut(&matter) {
                entry.turns.holder = Some(Holder::Thread(next));
            }
            steps::before_turn(actor, next, turn);
            return;
        }
    }
}
