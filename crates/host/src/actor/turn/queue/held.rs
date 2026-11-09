//! The queue's held flag and its reason on screen: `queue_held` and `queue_held_reason` in the
//! thread's header (and `ListThreads`). Both are the host's alone, read from its state each
//! time, never stored; nothing logs them.
//!
//! The queue shows held while turns wait in it and it is held for the lawyer (`held`, with or
//! without the sign-in's mark) or an install's hold applies (`hold.rs`): under that hold alone
//! queued turns wait too, the queue not held. The reason is the most specific that applies:
//!
//! 1. `switching`: `stop_agents` holds the agent while its login changes;
//! 2. `sign_in`: the queue is held for its agent's sign-in (`sign_in.rs`), and so is a queue
//!    under a `stop_agents_for(SignInAgain)` stop, when the account on the shared login changed;
//! 3. `update`: an install's hold applies;
//! 4. `limit`: the queue is held because its last turn stopped at the plan's limit;
//! 5. `paused`: any other hold for the lawyer (a cancel, an error, a restart).
//!
//! `limit` lives in memory only (`held_for_limit`): after a host restart a queue held by a limit
//! stop shows `paused`, so the app offers "Send next" there as for any hold of the lawyer's.

use clauseo_protocol::{QueueHoldReason, StreamId, ThreadId};

use super::super::super::Actor;
use super::super::super::streams::StreamSlot;
use super::super::idle::Pending;
use super::super::sign_in;
use super::{ThreadQueue, hold, queue};
use crate::agent::login_link::StopFor;
use crate::log_file::Cause;

/// What in `queue` keeps its thread's agent from idling (`idle.rs`): a turn held for the sign-in
/// (marked, or one ended for it and still to be put back), else any queued prompt, steer out, or
/// removal in flight.
pub(in crate::actor::turn) fn waits(queue: &ThreadQueue) -> Option<Pending> {
    if queue.sign_in.is_some() || queue.requeue.is_some() {
        return Some(Pending::Held);
    }
    let waits = !(queue.turns.is_empty() && queue.steers.is_empty() && queue.removing.is_empty());
    waits.then_some(Pending::Queued)
}

/// Marks a turn ended for the sign-in, still to be put back (`Some`), or none (`None`), with
/// nothing else in `queue`: the idle stop's test of the held check on its own (`idle.rs`).
#[cfg(test)]
pub(in crate::actor::turn) fn set_requeue_for_test(
    queue: &mut ThreadQueue,
    turn: Option<clauseo_protocol::TurnId>,
) {
    queue.requeue = turn.map(|turn| super::Requeue {
        turn,
        content: Vec::new(),
    });
}

/// `thread`'s agent was stopped for moving itself to a mode that approves without asking
/// (`forced_mode::unjailed::stop_unasked`): its queued turns wait for the lawyer (`paused`),
/// a head sent now included, so none starts the agent again by itself, even when the stopped
/// turn ends `EndTurn`. An empty queue is not held: the lawyer's next message starts a new agent.
pub(in crate::actor::turn) fn hold_for_stop(actor: &mut Actor, thread: ThreadId) {
    let Some(queue) = queue(actor, thread) else {
        return;
    };
    if queue.is_empty() {
        return;
    }
    queue.held = true;
    queue.held_for_limit = false;
    for queued in &mut queue.turns {
        queued.forced = false;
    }
    sync_held(actor, thread);
}

/// Why `thread`'s queue shows held, or `None` when it does not.
pub(crate) fn queue_hold(actor: &Actor, thread: ThreadId) -> Option<QueueHoldReason> {
    let queue = &actor.threads.get(&thread)?.turn.queue;
    if queue.is_empty() {
        return None;
    }
    let holds = hold::holds(actor, thread);
    if !(queue.held || holds.update_now()) {
        return None;
    }
    let reason = if holds.stop() {
        match sign_in::stop_for(actor, thread) {
            Some(StopFor::SignInAgain) => QueueHoldReason::SignIn,
            _ => QueueHoldReason::Switching,
        }
    } else if queue.held && queue.sign_in.is_some() {
        QueueHoldReason::SignIn
    } else if holds.update_now() {
        QueueHoldReason::Update
    } else if queue.held_for_limit {
        QueueHoldReason::Limit
    } else {
        QueueHoldReason::Paused
    };
    Some(reason)
}

/// Whether `thread`'s queue shows held ([`queue_hold`]).
#[cfg(test)]
pub(crate) fn queue_held(actor: &Actor, thread: ThreadId) -> bool {
    queue_hold(actor, thread).is_some()
}

/// Shows `queue_held` and its reason when either changed: through the loaded view, or in the
/// header kept for `ListThreads` when the stream is not loaded; the app's thread rows compare
/// them at the next flush. Called after every change to what [`queue_hold`] reads.
pub(in super::super) fn sync_held(actor: &mut Actor, thread: ThreadId) {
    let shown = queue_hold(actor, thread);
    let Some(queue) = queue(actor, thread) else {
        return;
    };
    if !queue.held || queue.is_empty() {
        // A queue held for the sign-in or the limit that moved on, or emptied, waits for it no
        // more.
        queue.sign_in = None;
        queue.held_for_limit = false;
    }
    if queue.shown == shown {
        return;
    }
    queue.shown.clone_from(&shown);
    // The row `ListThreads` and `ThreadsChanged` carry shows it too.
    actor.lists.touch_thread(thread);
    let stream = StreamId::Thread(thread);
    let Actor {
        threads,
        streams,
        encoder,
        ..
    } = actor;
    match streams.get_mut(&stream) {
        Some(StreamSlot::Loaded(loaded)) => {
            let change = loaded
                .state
                .as_thread_mut()
                .and_then(|state| clauseo_reducer::set_queue_held(state, shown));
            let Some(change) = change else {
                return;
            };
            match loaded.view.publish(vec![change], encoder) {
                Ok(encoded) => {
                    for event in &encoded {
                        loaded.views.retain_mut(|sub| sub.offer(event));
                    }
                }
                Err(error) => {
                    tracing::error!(%thread, error = %Cause(&error), "queue_held was not shown");
                }
            }
        }
        Some(StreamSlot::Loading(_)) | None => {
            if let Some(header) = threads
                .get_mut(&thread)
                .and_then(|entry| entry.header.as_mut())
            {
                header.queue_held = shown.is_some();
                header.queue_held_reason = shown;
            }
        }
    }
}

/// [`sync_held`] for every thread: after a change that reaches them all (an install's hold
/// starting or ending).
pub(crate) fn sync_all_held(actor: &mut Actor) {
    let threads: Vec<ThreadId> = actor.threads.keys().copied().collect();
    for thread in threads {
        sync_held(actor, thread);
    }
}
