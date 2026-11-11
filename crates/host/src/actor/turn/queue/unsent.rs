//! Steers the host held, at a stop. A steer that could not join the running turn now waits as
//! the next turn (`steer::next`), and so does one the host moved to a new turn before the agent
//! took it (`steer.rs`'s requeue, other than the lawyer's Send now): both start only once the
//! turn they wait behind has ended `EndTurn`, which is the case of a permission answered either
//! way and the turn carrying on to its end. When that turn ends with any other stop (Stop, Send
//! now of another message, a failure), they are not sent: each is taken out of the queue with
//! one commit of its turn's `TurnEnded { Cancelled }` (as `RemoveQueuedPrompt` logs it) and
//! `SteerResolved { NotSent }` naming the turn that ended. Queued prompts and heads sent now keep
//! the queue's own rules (`queue.rs`).

use clauseo_protocol::{
    SteerOutcome, SteerResolved, StopReason, StreamId, ThreadId, TurnEnded, TurnId,
};

use super::super::super::Actor;
use super::super::Then;
use super::super::out::Out;
use super::steer::why;
use super::{Queued, queue};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

/// The current turn `turn` ended with `stop`, durably: after any stop but `EndTurn`, the steers
/// the host held behind it leave the queue, not sent.
pub(super) fn turn_closed(actor: &mut Actor, thread: ThreadId, turn: TurnId, stop: StopReason) {
    if stop == StopReason::EndTurn {
        return;
    }
    let Some(queue) = queue(actor, thread) else {
        return;
    };
    let (held, kept): (Vec<Queued>, Vec<Queued>) = std::mem::take(&mut queue.turns)
        .into_iter()
        .partition(|queued| queued.host_held);
    queue.turns = kept.into();
    for queued in held {
        tracing::info!(target: LIFECYCLE, %thread, turn = %turn, why = why::TURN_STOPPED, "a steer was not sent");
        let stream = StreamId::Thread(thread);
        actor.with_loaded(
            stream,
            Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
                if let Err(error) = loaded.and_then(|()| log(actor, thread, turn, &queued)) {
                    tracing::warn!(%thread, error = %Cause(&error), "a held steer's end was not logged");
                }
                actor.release(stream);
            }),
        );
    }
}

/// One commit of `queued`'s `TurnEnded { Cancelled }` and `SteerResolved { NotSent }` naming
/// `ended`.
fn log(actor: &mut Actor, thread: ThreadId, ended: TurnId, queued: &Queued) -> Result<(), Error> {
    let mut out = Out::new(actor, thread, None)?;
    out.push(
        actor,
        TurnEnded {
            turn: queued.turn,
            stop: StopReason::Cancelled,
            usage: None,
            checkpoint_after: None,
        },
    )?;
    out.push(
        actor,
        SteerResolved {
            message: queued.message,
            outcome: SteerOutcome::NotSent { turn: ended },
            aside: Vec::new(),
        },
    )?;
    out.send(actor, Then::Nothing)
}
