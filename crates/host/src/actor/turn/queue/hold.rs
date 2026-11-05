//! Holds on a thread's new turns, and the one held append (decision records 0037 and 0048).
//! A hold applies to a thread for any of three reasons, a set ([`Holds`]) each of whose members
//! its own owner lifts:
//!
//! - `Stop`: `stop_agents` holds the thread's agent; `restart_agents` lifts it
//!   (`sign_in/stops.rs`).
//! - `SignIn`: the queue is held for its agent's sign-in (`sign_in.rs`); `restart_agents`
//!   lifts it, or the lawyer's `ResumeQueue` or `SendQueuedNow`.
//! - `UpdateNow`: an install under `now` holds new sends while its turns stop
//!   (`actor/update.rs`); the install going ahead or being taken back lifts it.
//!
//! Each member is read from its owner's state, so a lift removes only its own reason. A prompt
//! sent while any applies and nothing runs takes one path, [`hold_prompt`], and its commit one
//! arm, `After::Held`. Nothing starts while `Stop` or `UpdateNow` applies (`move_on`), and
//! `SignIn` holds through the queue's held flag, as any held queue does: a queued prompt stays
//! held while any hold applies, and runs once the last one is lifted.

use clauseo_counts::schema::Delivery;
use clauseo_protocol::{ContentBlock, DeliveryRequest, ThreadId};

use super::super::super::Actor;
use super::super::super::commands::CommandCtx;
use super::{After, Queued, log_queued, refuse_full, turn_ahead};
use crate::error::Error;

/// The holds that apply to a thread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Holds {
    stop: bool,
    sign_in: bool,
    update_now: bool,
}

impl Holds {
    /// No hold applies.
    pub(super) fn is_empty(self) -> bool {
        !(self.stop || self.sign_in || self.update_now)
    }

    /// An install's hold applies.
    pub(super) fn update_now(self) -> bool {
        self.update_now
    }

    /// A hold that keeps anything from starting applies: `Stop` or `UpdateNow`. `SignIn` holds
    /// through the queue's held flag instead, which a head sent now passes.
    pub(super) fn stops_starts(self) -> bool {
        self.stop || self.update_now
    }

    /// The most specific reason, for the log: the sign-in's, then the stop's, then the
    /// install's.
    fn reason(self) -> &'static str {
        if self.sign_in {
            "sign_in"
        } else if self.stop {
            "stop"
        } else if self.update_now {
            "update_now"
        } else {
            "none"
        }
    }
}

/// The holds that apply to `thread` now, each read from its owner.
pub(super) fn holds(actor: &Actor, thread: ThreadId) -> Holds {
    Holds {
        stop: super::super::sign_in::holds(actor, thread),
        sign_in: actor.threads.get(&thread).is_some_and(|entry| {
            let queue = &entry.turn.queue;
            queue.held && queue.sign_in.is_some()
        }),
        update_now: super::super::super::update::gated(actor),
    }
}

/// A prompt sent while `holds` apply and nothing runs, whatever its delivery: one commit of its
/// `UserMessage` and its `TurnQueued`, and the queue as the holds want it. Refused with
/// `thread.queue_full` at the limit.
///
/// - A sign-in's hold (`SignIn` or `Stop`), alone or with the install's: at the queue's end,
///   behind the last queued turn (behind the thread itself when none waits), and the queue held
///   for the sign-in, the first turn waiting marked unless one is.
/// - The install's alone: the place it would have had with nothing running. Sent now, it joins
///   the head, sent now, and the held queue moves on after it; queued, it joins the end and the
///   queue is no longer held. It starts once the hold lifts.
pub(super) fn hold_prompt(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    blocks: Vec<ContentBlock>,
    delivery: DeliveryRequest,
    holds: Holds,
) -> Result<(), Error> {
    refuse_full(actor, thread)?;
    tracing::info!(%thread, held_for = holds.reason(), "a prompt queues held");
    let sent_now = !holds.stop && !holds.sign_in && delivery != DeliveryRequest::Queue;
    let ahead = if sent_now {
        None
    } else {
        turn_ahead(actor, thread)
    };
    let logged = if sent_now {
        delivery
    } else {
        DeliveryRequest::Queue
    };
    log_queued(
        actor,
        ctx,
        thread,
        ahead,
        blocks,
        logged,
        |turn, message, content, queue| {
            let queued = Queued {
                turn,
                message,
                content,
                forced: sent_now,
            };
            if holds.stop || holds.sign_in {
                queue.turns.push_back(queued);
                queue.held = true;
                let first = queue.turns.front().map_or(turn, |head| head.turn);
                queue.sign_in.get_or_insert(first);
                return After::Held(turn, Delivery::Queued);
            }
            queue.held = false;
            if sent_now {
                queue.turns.push_front(queued);
                After::Held(turn, Delivery::Now)
            } else {
                queue.turns.push_back(queued);
                After::Held(turn, Delivery::Queued)
            }
        },
    )
}
