//! A queued switch held back by background work (`switch.rs` step 2): its turn goes back to
//! the head of the held queue, marked, and the end of the last open helper (its own end, a Stop
//! or a detach) resumes it once nothing else counts as background work (`switch::background`).
//! Only this mark is lifted: a queue held by the lawyer, a turn's stop or a sign-in, or under any
//! hold (`hold.rs`), stays held. The mark goes when the queue is no longer held or empties
//! (`held.rs`), when another turn takes the head, and when a turn's stop holds the queue again.

use clauseo_protocol::ThreadId;
use clauseo_protocol::TurnId;

use super::super::super::Actor;
use super::super::state;
use super::{hold, move_on, put_back, queue, sync_held};
use crate::log_file::LIFECYCLE;

/// Puts `turn`, a switch that met background work, back at the head of the held queue, marked
/// for [`background_ended`].
pub(in crate::actor::turn) fn put_back_for_background(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
) {
    put_back(actor, thread, turn);
    if let Some(queue) = queue(actor, thread)
        && queue.turns.front().is_some_and(|head| head.turn == turn)
    {
        queue.background = Some(turn);
    }
}

/// A helper of `thread` ended (or a turn closed): a queue this module marked resumes when its
/// marked turn is still the head, no hold applies, nothing runs or closes, and no background
/// work is left. Costs one field read for a queue not marked.
pub(in crate::actor::turn) fn background_ended(actor: &mut Actor, thread: ThreadId) {
    let marked = queue(actor, thread).is_some_and(|queue| {
        queue.held
            && queue.background.is_some()
            && queue.turns.front().map(|head| head.turn) == queue.background
    });
    if !marked || !hold::holds(actor, thread).is_empty() {
        return;
    }
    let busy =
        state(actor, thread).is_none_or(|state| state.current.is_some() || state.closing.is_some());
    if busy || super::super::switch::background(actor, thread) {
        return;
    }
    if let Some(queue) = queue(actor, thread) {
        queue.background = None;
        queue.held = false;
    }
    tracing::info!(target: LIFECYCLE, %thread, "the background work a queued switch waited for ended; the queue resumes");
    sync_held(actor, thread);
    move_on(actor, thread);
}
