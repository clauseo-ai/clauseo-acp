//! A steer moved to a new turn at the queue's head (`Requeued`): its answer was not
//! `injected`, it got no answer in time while its turn ran, it could not be sent, or the lawyer
//! sent it now.

use clauseo_protocol::{StreamId, ThreadId, TurnId, TurnQueued};

use super::super::super::super::Actor;
use super::super::super::out::Out;
use super::super::super::{Then, lost};
use super::super::{After, Queued, drop_turn, move_on, queue, sync_held};
use super::{Ask, requeued, why};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

/// `asks`, in the order they were sent, each move to a new turn at the head, the first sent
/// first.
pub(super) fn requeue_all(actor: &mut Actor, thread: ThreadId, asks: Vec<Ask>, why: &'static str) {
    for ask in asks.into_iter().rev() {
        requeue(actor, thread, ask, why);
    }
}

/// The steer moves to a new turn at the queue's head: one commit of `SteerResolved { Requeued }`
/// and `TurnQueued` behind the steered turn, marked `at_head` so a restart keeps it at the head
/// too (`restore.rs`). Sent now by the lawyer, it starts once the steered turn has ended,
/// whatever its stop; otherwise the host holds it, and it starts only after `EndTurn`
/// (`unsent.rs`).
pub(super) fn requeue(actor: &mut Actor, thread: ThreadId, ask: Ask, why: &'static str) {
    tracing::info!(target: LIFECYCLE, %thread, turn = %ask.turn, why, "a steer was requeued");
    let host_held = why != why::SEND_NOW;
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let logged = loaded.and_then(|()| log_requeue(actor, thread, ask, host_held));
            if let Err(error) = logged {
                tracing::warn!(
                    %thread,
                    error = %Cause(&error),
                    "a steer was not moved to a new turn"
                );
            }
            move_on(actor, thread);
            sync_held(actor, thread);
            actor.release(stream);
        }),
    );
}

fn log_requeue(
    actor: &mut Actor,
    thread: ThreadId,
    ask: Ask,
    host_held: bool,
) -> Result<(), Error> {
    let now = actor.now().wall;
    let turn: TurnId = actor.ids.mint(now)?;
    // `at_head`: the live queue puts it at the head, and a restart does the same.
    let waits = TurnQueued::new(thread, turn, thread, Some(ask.turn))
        .map_err(|error| Error::Internal(error.to_string()))?
        .at_head();
    let mut out = Out::new(actor, thread, None)?;
    out.push(actor, requeued(ask.message, turn))?;
    out.push(actor, waits)?;
    queue(actor, thread)
        .ok_or_else(|| lost("a thread"))?
        .turns
        .push_front(Queued {
            turn,
            message: ask.message,
            content: ask.content,
            forced: true,
            host_held,
        });
    let sent = out.send(actor, Then::Queue(After::Requeued(turn)));
    if sent.is_err() {
        drop_turn(actor, thread, turn);
    }
    sent
}
