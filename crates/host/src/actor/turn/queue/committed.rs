//! A queue command's commit returned: what the queue does next ([`committed`]), and the turn a
//! refused commit takes back out ([`drop_turn`]).

use clauseo_counts::schema::Delivery;
use clauseo_protocol::{ThreadId, TurnId};

use super::super::super::{Actor, counting};
use super::super::cancel;
use super::commands::{self, put_content};
use super::{After, move_on, queue, sign_in, steer, sync_held};

/// Takes `turn` out of the queue (its commit was refused).
pub(in crate::actor::turn) fn drop_turn(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if let Some(queue) = queue(actor, thread)
        && let Some(at) = queue.position(turn)
    {
        queue.turns.remove(at);
    }
    sync_held(actor, thread);
}

/// A queue commit returned.
pub(in crate::actor::turn) fn committed(
    actor: &mut Actor,
    thread: ThreadId,
    after: After,
    ok: bool,
) {
    match (after, ok) {
        (After::Appended(_), true) => {
            counting::question_sent(actor, thread, Delivery::Queued);
            // A hold for the sign-in keeps the queue held, and its mark.
            if !sign_in::applies(actor, thread)
                && let Some(queue) = queue(actor, thread)
            {
                queue.held = false;
            }
            move_on(actor, thread);
        }
        (After::Jumped(_), true) => {
            // Sent now, over the running answer, which it cancels.
            counting::question_sent(actor, thread, Delivery::Now);
            cancel::cancel_current(actor, thread, cancel::why::SEND_NOW);
            move_on(actor, thread);
        }
        (After::Requeued(turn), true) => {
            sign_in::copied(actor, thread, turn);
            move_on(actor, thread);
        }
        (After::Edited { .. }, true) => move_on(actor, thread),
        (After::Waits(_), true) => {
            counting::question_sent(actor, thread, Delivery::Steer);
            move_on(actor, thread);
        }
        (After::Steer(ask), true) => {
            counting::question_sent(actor, thread, Delivery::Steer);
            steer::send(actor, thread, *ask);
        }
        (After::Removed(turn), ok) => commands::removed(actor, thread, turn, ok),
        (
            After::Held {
                delivery, unhold, ..
            },
            true,
        ) => {
            counting::question_sent(actor, thread, delivery);
            // Only now, so a refused commit leaves a held queue held (`hold.rs`).
            if unhold
                && !sign_in::applies(actor, thread)
                && let Some(queue) = queue(actor, thread)
            {
                queue.held = false;
            }
            // Nothing starts while the install's hold applies; taken back meanwhile, the
            // queue moves on now.
            move_on(actor, thread);
        }
        (
            After::Appended(turn)
            | After::Jumped(turn)
            | After::Requeued(turn)
            | After::Waits(turn)
            | After::Held { turn, .. },
            false,
        ) => {
            sign_in::copied(actor, thread, turn);
            drop_turn(actor, thread, turn);
        }
        (After::Edited { turn, old }, false) => put_content(actor, thread, turn, old),
        (After::Steer(_), false) => {}
    }
    sync_held(actor, thread);
}
