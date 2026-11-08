//! A turn held for its agent's sign-in (R20 brief, "The expired sign-in"). A turn that ends
//! failed with `agent.auth_required`, or that `HostHandle::stop_agents` cancelled and that did
//! not end `EndTurn`, goes back to the head of its thread's queue, held: once its close is
//! durable, one commit of a new `UserMessage { Queue }` with the same content and its
//! `TurnQueued` behind the ended turn, the same entries a queued prompt has, the `TurnQueued`
//! marked `at_head` so a restart keeps it first. The queue is then
//! held exactly as after any other stop but `EndTurn` (a refused permission's `NotAllowed`
//! included), so `queue_held` and the queued prompt look the same whatever held it, and it is
//! marked as waiting for the sign-in. `HostHandle::restart_agents` resumes it once; a lawyer's
//! `ResumeQueue` or `SendQueuedNow` moves it as usual, and the mark goes when the queue is no
//! longer held or empties (`held.rs`).
//!
//! While the queue is held for the sign-in, or `HostHandle::stop_agents` holds the agent
//! (decision record 0048, P4d), a hold applies ([`applies`]): a prompt sent while nothing runs
//! queues at the end, held and marked the same way, whatever its delivery (queue, send now, or a
//! steer with nothing to join), so it is not lost and nothing launches (`hold.rs`, which an
//! install's hold shares); a prompt that joins the queue keeps it held and marked. Under
//! `stop_agents`' hold a lawyer's `ResumeQueue` or `SendQueuedNow` changes nothing
//! (`commands.rs`). `restart_agents` lifts the hold and resumes the queue once, through
//! [`resume`].

use clauseo_protocol::TurnQueued;
use clauseo_protocol::{ContentBlock, DeliveryRequest, MessageId, StreamId, ThreadId, TurnId};

use super::super::super::Actor;
use super::super::out::Out;
use super::super::{Then, host_stopping, lost};
use super::{After, Queued, drop_turn, move_on, queue, sync_held};
use crate::build;
use crate::error::Error;

/// The turn to put back once its close is durable.
#[derive(Debug)]
pub(in crate::actor::turn) struct Requeue {
    /// The ended turn.
    pub(in crate::actor::turn) turn: TurnId,
    /// Its prompt, as it was sent.
    pub(in crate::actor::turn) content: Vec<ContentBlock>,
}

/// `requeue`'s turn has ended and its close is on its way: it goes back once that is durable.
pub(in crate::actor::turn) fn keep(actor: &mut Actor, thread: ThreadId, requeue: Requeue) {
    if host_stopping(actor) {
        return;
    }
    if let Some(queue) = queue(actor, thread) {
        queue.requeue = Some(requeue);
    }
}

/// The close of `turn` is durable: a turn kept for it goes back to the queue's head, held.
pub(super) fn on_closed(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(requeue) = queue(actor, thread)
        .and_then(|queue| queue.requeue.take_if(|requeue| requeue.turn == turn))
    else {
        return;
    };
    if host_stopping(actor) {
        return;
    }
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            if let Err(error) = loaded.and_then(|()| log(actor, thread, requeue)) {
                tracing::warn!(%thread, %error, "a turn held for the sign-in was not put back");
            }
            move_on(actor, thread);
            sync_held(actor, thread);
            actor.release(stream);
        }),
    );
}

/// One commit of the new turn's `UserMessage { Queue }` and its `TurnQueued` behind the ended
/// turn; the new turn waits behind any turn sent now, ahead of the rest, held.
fn log(actor: &mut Actor, thread: ThreadId, requeue: Requeue) -> Result<(), Error> {
    let now = actor.now().wall;
    let turn: TurnId = actor.ids.mint(now)?;
    let message: MessageId = actor.ids.mint(now)?;
    let delivery = DeliveryRequest::Queue.into();
    let built = build::user_message(turn, message, None, requeue.content.clone(), delivery);
    // `at_head`: a restart puts it back ahead of the turns queued before it (`restore.rs`).
    let waits = TurnQueued::new(thread, turn, thread, Some(requeue.turn))
        .map_err(|error| Error::Internal(error.to_string()))?
        .at_head();
    let mut out = Out::new(actor, thread, None)?;
    out.push(actor, built.value)?;
    out.draft.blobs(built.blobs);
    out.push(actor, waits)?;
    let queue = queue(actor, thread).ok_or_else(|| lost("a thread"))?;
    let at = queue
        .turns
        .iter()
        .take_while(|queued| queued.forced)
        .count();
    queue.turns.insert(
        at,
        Queued {
            turn,
            message,
            content: requeue.content,
            forced: false,
        },
    );
    queue.held = true;
    queue.held_for_limit = false;
    queue.sign_in = Some(turn);
    tracing::info!(%thread, "a turn is held for its agent's sign-in");
    let sent = out.send(actor, Then::Queue(After::Requeued(turn)));
    if sent.is_err() {
        drop_turn(actor, thread, turn);
    }
    sent
}

/// Whether a hold for the sign-in applies to `thread`: `stop_agents` holds its agent, or its
/// queue is held for the sign-in. Nothing starts until `restart_agents` (or the lawyer, for the
/// second), so a prompt queues held.
pub(super) fn applies(actor: &Actor, thread: ThreadId) -> bool {
    super::hold::holds(actor, thread).sign_ins()
}

/// `restart_agents`' part for `thread`: a queue held for the sign-in moves on, once.
pub(in crate::actor::turn) fn resume(actor: &mut Actor, thread: ThreadId) {
    let Some(queue) = queue(actor, thread) else {
        return;
    };
    if !(queue.sign_in.is_some() && queue.held) {
        return;
    }
    queue.sign_in = None;
    queue.held = false;
    tracing::info!(%thread, "a queue held for the sign-in resumes");
    sync_held(actor, thread);
    move_on(actor, thread);
}
