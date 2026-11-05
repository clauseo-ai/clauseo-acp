//! A thread's queue: the prompts sent while a turn runs wait here, in order, until the turn
//! ends, then start one at a time at step 2.
//!
//! A queued turn has its `UserMessage` and a `TurnQueued` behind its own thread logged, and no
//! `TurnStarted` or `TurnEnded`. The queue moves on when the current turn's `TurnEnded` is
//! durable: always to a head sent now (send now, a queued prompt sent now, a requeued steer), else
//! only after `EndTurn`. Any other stop (`Cancelled`, `NotAllowed`, `Failed`, `AtLimit`,
//! `Refusal`, `MaxTokens`, `MaxTurnRequests`) holds it until the lawyer moves it: `ResumeQueue`,
//! `SendQueuedNow` or a new `SendPrompt`. While a steer request is out, it waits for the answer,
//! so a requeued steer takes the head. The held flag lives here only (nothing is logged for it);
//! the view shows it through `clauseo_reducer::set_queue_held`. At start-up every queued turn
//! comes back held, rebuilt from the log ([`restore`]).

mod commands;
mod held;
mod restore;
mod sign_in;
mod steer;

use std::collections::VecDeque;

use clauseo_counts::schema::Delivery;
use clauseo_protocol::{
    CommandOutcome, ContentBlock, DeliveryRequest, ErrorCode, MAX_QUEUED_TURNS, MessageId,
    PromptBlock, StopReason, StreamId, ThreadId, TurnId, TurnQueued,
};

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::{Actor, Tag, counting};
use super::out::Out;
use super::{Current, Holder, Then, TurnTag, cancel, host_stopping, lost, matter_of, state, steps};
use crate::build;
use crate::error::Error;
use crate::log_file::Cause;

use commands::put_content;
pub(super) use commands::{edit, remove, resume, send_now};
pub(crate) use held::queue_held;
pub(super) use held::sync_held;
pub(super) use restore::restore;
pub(super) use sign_in::{Requeue, keep as keep_for_sign_in, resume as resume_sign_in};
pub(super) use steer::{Steered, on_steer_timer, on_steered};

/// A thread's queued turns, in the order they start, and what moves them.
#[derive(Debug, Default)]
pub(crate) struct ThreadQueue {
    turns: VecDeque<Queued>,
    /// The queue waits for the lawyer: the last turn ended with a stop other than `EndTurn`,
    /// or the host started again.
    held: bool,
    /// Real steer requests not answered yet; the queue waits for them.
    steers: Vec<steer::Pending>,
    /// A current turn the lawyer removed before it started: its end neither holds nor moves
    /// the queue differently from the turn before it.
    removed: Option<TurnId>,
    /// The `queue_held` last shown.
    shown: bool,
    /// The queue is held for its agent's sign-in: `restart_agents` resumes it once
    /// (`sign_in.rs`). Cleared whenever the queue no longer shows held.
    sign_in: bool,
    /// A turn that ended for the sign-in, put back once its close is durable.
    requeue: Option<Requeue>,
    /// Queued turns whose `RemoveQueuedPrompt` commit is in flight, with their place: gone
    /// once it is durable, back in their place if it fails, and never started meanwhile.
    removing: Vec<(usize, Queued)>,
}

/// A queued turn: its prompt, as step 5 will send it.
#[derive(Debug)]
pub(super) struct Queued {
    pub(super) turn: TurnId,
    pub(super) message: MessageId,
    pub(super) content: Vec<ContentBlock>,
    /// It starts when the current turn has ended, whatever its stop.
    pub(super) forced: bool,
}

impl ThreadQueue {
    /// No queued turn.
    pub(super) fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }

    fn position(&self, turn: TurnId) -> Option<usize> {
        self.turns.iter().position(|queued| queued.turn == turn)
    }
}

/// The queued turns of `thread`, in order.
#[cfg(test)]
pub(super) fn queued_turns(actor: &Actor, thread: ThreadId) -> Vec<TurnId> {
    actor.threads.get(&thread).map_or_else(Vec::new, |entry| {
        entry
            .turn
            .queue
            .turns
            .iter()
            .map(|queued| queued.turn)
            .collect()
    })
}

/// What a queue command's commit continues with.
#[derive(Debug)]
pub(crate) enum After {
    /// A prompt joined the queue's end: the queue is no longer held.
    Appended(TurnId),
    /// A prompt sent now joined its head: cancel the current turn.
    Jumped(TurnId),
    /// A queued prompt was edited; `old` is put back if the commit fails.
    Edited {
        turn: TurnId,
        old: Vec<ContentBlock>,
    },
    /// A steer is durable: send it (`steer.rs`).
    Steer(Box<steer::Ask>),
    /// A requeued steer's turn joined the head.
    Requeued(TurnId),
    /// A queued turn's removal (`TurnEnded { Cancelled }`).
    Removed(TurnId),
}

pub(super) fn queue(actor: &mut Actor, thread: ThreadId) -> Option<&mut ThreadQueue> {
    state(actor, thread).map(|state| &mut state.queue)
}

/// `SendPrompt`: starts now, queues, sends now or steers, by the delivery and the thread's state.
pub(super) fn send_prompt(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    content: Vec<PromptBlock>,
    delivery: DeliveryRequest,
) -> Result<(), Error> {
    steps::refuse_unsendable(actor, thread)?;
    let blocks = steps::prompt_blocks(actor, thread, content)?;
    super::logout::refuse(actor, thread, &blocks)?;
    with_stream(actor, ctx, thread, move |actor| {
        route(actor, ctx, thread, blocks, delivery)
    });
    Ok(())
}

/// Runs a command's `work` once `thread`'s stream is loaded (its events need the stream's
/// head); a failure finishes the command.
pub(super) fn with_stream(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    work: impl FnOnce(&mut Actor) -> Result<(), Error> + 'static,
) {
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            if let Err(error) = loaded.and_then(|()| work(actor)) {
                actor.finish_command(ctx.id, Err(error));
            }
            actor.release(stream);
        }),
    );
}

/// The turn a new queued turn waits behind: the last queued, else the current turn, else the
/// last turn whose end is not durable yet.
fn turn_ahead(actor: &mut Actor, thread: ThreadId) -> Option<TurnId> {
    let state = state(actor, thread)?;
    state
        .queue
        .turns
        .back()
        .map(|queued| queued.turn)
        .or_else(|| state.current.as_ref().map(|current| current.turn))
        .or_else(|| state.closing.as_ref().map(|closing| closing.close.turn))
}

/// The turn a send now cancels or waits for: the current turn, else the closing one.
fn turn_running(actor: &mut Actor, thread: ThreadId) -> Option<TurnId> {
    let state = state(actor, thread)?;
    state
        .current
        .as_ref()
        .map(|current| current.turn)
        .or_else(|| state.closing.as_ref().map(|closing| closing.close.turn))
}

fn route(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    blocks: Vec<ContentBlock>,
    delivery: DeliveryRequest,
) -> Result<(), Error> {
    steps::refuse_unsendable(actor, thread)?;
    refuse_stuck(actor, thread)?;
    if delivery == DeliveryRequest::Steer
        && let Some(target) = steer::target(actor, thread)
    {
        return steer::steer(actor, ctx, thread, target, blocks);
    }
    let running = turn_running(actor, thread);
    let queued = queue(actor, thread).is_some_and(|queue| !queue.is_empty());
    match (delivery, running) {
        (DeliveryRequest::Queue, None) if !queued => {
            if let Some(queue) = queue(actor, thread) {
                queue.held = false;
            }
            steps::accept(actor, ctx, thread, blocks, delivery.into())
        }
        (DeliveryRequest::Queue, _) => append(actor, ctx, thread, blocks),
        (DeliveryRequest::SendNow | DeliveryRequest::Steer, Some(running)) => {
            jump(actor, ctx, thread, running, blocks, delivery)
        }
        (DeliveryRequest::SendNow | DeliveryRequest::Steer, None) => {
            // It starts at once, ahead of a held queue, which moves on after it.
            if let Some(queue) = queue(actor, thread) {
                queue.held = false;
            }
            sync_held(actor, thread);
            steps::accept(actor, ctx, thread, blocks, delivery.into())
        }
    }
}

/// `thread.turn_running` while the last turn's end failed twice: nothing starts until the next
/// start ends that turn, so a queued prompt would wait unseen.
fn refuse_stuck(actor: &mut Actor, thread: ThreadId) -> Result<(), Error> {
    let stuck = state(actor, thread)
        .and_then(|state| state.closing.as_ref())
        .is_some_and(|closing| closing.retried);
    if stuck {
        return Err(Error::refused(
            ErrorCode::ThreadTurnRunning,
            "the last turn of this thread could not be saved; it ends at the next start",
        ));
    }
    Ok(())
}

/// A prompt joins the end of the queue: one commit of `UserMessage { Queue }`, `TurnQueued`
/// behind the turn ahead, and the receipt. Refused with `thread.queue_full` at the limit.
fn append(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    blocks: Vec<ContentBlock>,
) -> Result<(), Error> {
    let full = queue(actor, thread).is_some_and(|queue| queue.turns.len() >= MAX_QUEUED_TURNS);
    if full {
        return Err(Error::refused(
            ErrorCode::ThreadQueueFull,
            format!("the thread already has {MAX_QUEUED_TURNS} queued messages"),
        ));
    }
    let ahead = turn_ahead(actor, thread).ok_or_else(|| lost("the turn ahead"))?;
    let queued = |turn, message, content| Queued {
        turn,
        message,
        content,
        forced: false,
    };
    log_queued(
        actor,
        ctx,
        thread,
        ahead,
        blocks,
        DeliveryRequest::Queue,
        |turn, message, content, queue| {
            queue.turns.push_back(queued(turn, message, content));
            After::Appended(turn)
        },
    )
}

/// A prompt sent now while a turn runs or ends: it joins the head and starts once that turn
/// has ended, whatever its stop; the turn is cancelled once the commit is durable.
fn jump(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    running: TurnId,
    blocks: Vec<ContentBlock>,
    delivery: DeliveryRequest,
) -> Result<(), Error> {
    log_queued(
        actor,
        ctx,
        thread,
        running,
        blocks,
        delivery,
        |turn, message, content, queue| {
            queue.turns.push_front(Queued {
                turn,
                message,
                content,
                forced: true,
            });
            After::Jumped(turn)
        },
    )
}

/// One commit of a new turn's `UserMessage`, its `TurnQueued` behind `ahead` of its own
/// thread, `SteerResolved { SentNow }` for a steer, and the receipt (`PromptAccepted`). `place`
/// puts the turn in the queue before the commit is sent (taken out again if it is refused).
fn log_queued(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    ahead: TurnId,
    blocks: Vec<ContentBlock>,
    delivery: DeliveryRequest,
    place: impl FnOnce(TurnId, MessageId, Vec<ContentBlock>, &mut ThreadQueue) -> After,
) -> Result<(), Error> {
    let now = actor.now().wall;
    let turn: TurnId = actor.ids.mint(now)?;
    let message: MessageId = actor.ids.mint(now)?;
    let built = build::user_message(turn, message, None, blocks.clone(), delivery.into());
    let waits = TurnQueued::new(thread, turn, thread, Some(ahead))
        .map_err(|error| Error::Internal(error.to_string()))?;
    let outcome = CommandOutcome::PromptAccepted { turn, message };
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    out.push(actor, built.value)?;
    out.draft.blobs(built.blobs);
    out.push(actor, waits)?;
    if delivery == DeliveryRequest::Steer {
        out.push(actor, steer::sent_now(message, turn))?;
    }
    out.draft
        .receipt(ctx.receipt(Some(StreamId::Thread(thread)), outcome, now));
    let after = place(
        turn,
        message,
        blocks,
        queue(actor, thread).ok_or_else(|| lost("a thread"))?,
    );
    let tag = command_tag(ctx, outcome, thread, after);
    let sent = out.send_tagged(actor, tag);
    if sent.is_err() {
        drop_turn(actor, thread, turn);
    }
    sent
}

pub(super) fn command_tag(
    ctx: CommandCtx,
    outcome: CommandOutcome,
    thread: ThreadId,
    after: After,
) -> Tag {
    Tag::Command {
        id: ctx.id,
        outcome,
        after: AfterCommand::Turn(TurnTag {
            thread,
            then: Then::Queue(after),
        }),
    }
}

/// Takes `turn` out of the queue (its commit was refused).
pub(super) fn drop_turn(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if let Some(queue) = queue(actor, thread)
        && let Some(at) = queue.position(turn)
    {
        queue.turns.remove(at);
    }
    sync_held(actor, thread);
}

/// A queue commit returned.
pub(super) fn committed(actor: &mut Actor, thread: ThreadId, after: After, ok: bool) {
    match (after, ok) {
        (After::Appended(_), true) => {
            counting::question_sent(actor, thread, Delivery::Queued);
            if let Some(queue) = queue(actor, thread) {
                queue.held = false;
            }
            move_on(actor, thread);
        }
        (After::Jumped(_), true) => {
            // Sent now, over the running answer, which it cancels.
            counting::question_sent(actor, thread, Delivery::Now);
            cancel::cancel_current(actor, thread);
            move_on(actor, thread);
        }
        (After::Requeued(_) | After::Edited { .. }, true) => move_on(actor, thread),
        (After::Steer(ask), true) => {
            counting::question_sent(actor, thread, Delivery::Steer);
            steer::send(actor, thread, *ask);
        }
        (After::Removed(turn), ok) => commands::removed(actor, thread, turn, ok),
        (After::Appended(turn) | After::Jumped(turn) | After::Requeued(turn), false) => {
            drop_turn(actor, thread, turn);
        }
        (After::Edited { turn, old }, false) => put_content(actor, thread, turn, old),
        (After::Steer(_), false) => {}
    }
    sync_held(actor, thread);
}

/// The commit ending the current turn `turn` is durable: a head sent now starts; else the
/// queue moves on after `EndTurn` (or a removed turn) and holds after any other stop.
pub(super) fn on_turn_closed(actor: &mut Actor, thread: ThreadId, turn: TurnId, stop: StopReason) {
    // A steer of this turn still unanswered starts its wait now.
    steer::turn_closed(actor, thread, turn);
    let Some(queue) = queue(actor, thread) else {
        return;
    };
    let removed = queue.removed.take_if(|removed| *removed == turn).is_some();
    let forced = queue.turns.front().is_some_and(|head| head.forced);
    if queue.is_empty() {
        // Nothing waits: the next prompt starts at once, and a queue it starts moves on.
        queue.held = false;
    } else if !forced && !removed && stop != StopReason::EndTurn {
        queue.held = true;
    }
    move_on(actor, thread);
    sync_held(actor, thread);
    sign_in::on_closed(actor, thread, turn);
}

/// Starts the head when nothing runs, no end or steer is outstanding, and the queue is not
/// held (a head sent now starts even then).
pub(super) fn move_on(actor: &mut Actor, thread: ThreadId) {
    if host_stopping(actor) {
        return;
    }
    let Some(state) = state(actor, thread) else {
        return;
    };
    let queue = &state.queue;
    let idle = state.current.is_none() && state.closing.is_none() && queue.steers.is_empty();
    let starts = queue
        .turns
        .front()
        .is_some_and(|head| head.forced || !queue.held);
    if idle && starts && !own_undo(actor, thread) {
        start_head(actor, thread);
    }
}

/// Whether the thread holds its matter's writer with no turn: its undo runs. A turn started
/// now would take that writer as its own and go out before the undo's note; the undo's end
/// moves the queue on instead (`undo.rs`).
fn own_undo(actor: &Actor, thread: ThreadId) -> bool {
    matter_of(actor, thread)
        .ok()
        .and_then(|matter| actor.matters.get(&matter))
        .is_some_and(|entry| entry.turns.holder == Some(Holder::Thread(thread)))
}

/// The head becomes the current turn and goes to step 2.
fn start_head(actor: &mut Actor, thread: ThreadId) {
    let started = actor.now().mono;
    let Some(state) = state(actor, thread) else {
        return;
    };
    let Some(head) = state.queue.turns.pop_front() else {
        return;
    };
    let turn = head.turn;
    state.current = Some(Box::new(Current::new(
        turn,
        head.message,
        head.content,
        started,
    )));
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            match loaded {
                Ok(()) => steps::on_accepted(actor, thread, turn),
                Err(error) => {
                    // Nothing could be logged for it: it waits again, held.
                    tracing::warn!(
                        %thread,
                        error = %Cause(&error),
                        "a queued turn could not start"
                    );
                    put_back(actor, thread, turn);
                }
            }
            actor.release(stream);
        }),
    );
    sync_held(actor, thread);
}

/// A head whose stream did not load goes back to the head of a held queue.
fn put_back(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    let Some(current) = state.current.take_if(|current| current.turn == turn) else {
        return;
    };
    let Current {
        turn,
        message,
        prompt,
        ..
    } = *current;
    state.queue.turns.push_front(Queued {
        turn,
        message,
        content: prompt,
        forced: false,
    });
    state.queue.held = true;
    sync_held(actor, thread);
}
