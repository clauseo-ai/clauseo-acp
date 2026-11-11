//! A real steer: the lawyer's message joins the running turn of an agent whose steer adds to
//! the answer (both of `SteeringCapability`'s flags, claude-agent-acp), only while the agent can
//! take it now: the turn is prompting, not cancelled, and no permission or question is open
//! (claude-agent-acp defers a steer while one is, and a cancel can then drop it). The message is
//! logged with the running turn before it is sent; the agent's answer decides where it went.
//! `injected` means only that the agent accepted it into its own queue: a Claude agent's steer
//! counts as delivered once its transcript shows it taken in (`confirm.rs`), any other agent's
//! at once. `promptRequired` (the turn had ended), `failed` or an error move it to a new turn
//! at the queue's head (`Requeued`), which starts next once the steered turn has ended
//! `EndTurn`. An answer that does not come within [`STEER_WAIT`] of the send moves it there too
//! while the turn runs, so an agent that never answers cannot freeze the queue; once that turn
//! has ended, no answer leaves it not sent.
//!
//! The steered turn's end settles the rest (`turn_closed`): after `EndTurn` a steer still
//! unanswered waits for its answer within its wait, and one accepted gets its checks after the
//! end; after any other stop an unanswered one is `NotSent` at once. Nothing the host has not
//! seen taken in is sent again on its own: the lawyer's Send now on the steered turn
//! (`send_now`) moves its unsettled steers to a new turn that starts next and cancels the turn.
//! A steer this agent's steer would join but that cannot join now waits, held by the host, as
//! the next turn ([`next`]); every other steer is a send now. Every steer is logged at info on
//! the lifecycle target as it is sent, answered, requeued, confirmed or not sent, with no text.

use clauseo_acp::SteerResult;
use clauseo_protocol::{
    AgentKind, CommandOutcome, ContentBlock, Delivery, MessageId, SteerOutcome, SteerResolved,
    StopReason, StreamId, ThreadId, Timestamp, TurnId,
};

use super::super::super::Actor;
use super::super::super::commands::CommandCtx;
use super::super::super::input::Input;
use super::super::agents::Stage;
use super::super::out::Out;
use super::super::{AgentReply, TimerKind, TurnInput, TurnPhase, TurnTimer, cancel, spawn, state};
use super::queued::{Joins, log_queued};
use super::{After, Queued, command_tag, confirm, move_on, queue, sync_held};
use crate::build;
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

mod requeue;

use requeue::{requeue, requeue_all};

/// How long a steer waits for the agent's answer, from its send, before it moves to a new turn.
const STEER_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// Why a steer moved to a new turn, as the lifecycle log names it.
pub(super) mod why {
    /// The agent could not take it now (a permission or question open, the turn not
    /// prompting): it waits as the next turn, never sent as a steer.
    pub(in super::super) const CANNOT_JOIN_NOW: &str = "cannot_join_now";
    /// The agent answered `promptRequired`: the turn had ended.
    pub(in super::super) const PROMPT_REQUIRED: &str = "prompt_required";
    /// The agent answered `failed`, or the request failed.
    pub(in super::super) const FAILED: &str = "failed";
    /// The agent started a turn of its own for it, which was cancelled.
    pub(in super::super) const STARTED_NEW_TURN: &str = "started_new_turn";
    /// No answer within [`super::STEER_WAIT`].
    pub(in super::super) const NO_ANSWER: &str = "no_answer";
    /// Not sent: its turn, or the turn it waited behind, ended with a stop other than
    /// `EndTurn` before the agent answered or before it started.
    pub(in super::super) const TURN_STOPPED: &str = "turn_stopped";
    /// Not sent: its turn ended and the agent never answered, or Claude's transcript never
    /// showed it taken in.
    pub(in super::super) const NO_CONFIRMATION: &str = "no_confirmation";
    /// Not sent: the host stopped before it was settled.
    pub(in super::super) const RESTART: &str = "restart";
    /// The lawyer sent it now.
    pub(in super::super) const SEND_NOW: &str = "send_now";
    /// No attached agent to send it to.
    pub(in super::super) const NO_AGENT: &str = "no_agent";
}

/// A steer sent and not answered yet, with its answer's deadline.
#[derive(Debug)]
pub(crate) struct Pending {
    ask: Ask,
    timer: Option<futures::future::AbortHandle>,
}

/// A steer, once its message is durable.
#[derive(Debug)]
pub(crate) struct Ask {
    /// The running turn it was sent into.
    pub(super) turn: TurnId,
    pub(super) message: MessageId,
    pub(super) content: Vec<ContentBlock>,
    /// When its message was logged.
    pub(super) sent: Timestamp,
    /// Claude's transcripts, marked as the steer went out (`confirm::mark`).
    pub(super) places: Vec<super::transcript::Place>,
}

/// The agent's answer to a steer.
#[derive(Debug)]
pub(crate) struct Steered {
    message: MessageId,
    result: Result<SteerResult, clauseo_acp::Error>,
}

impl Steered {
    /// The steer came back `Closed`: the agent's connection is gone.
    pub(in crate::actor::turn) fn closed(&self) -> bool {
        matches!(self.result, Err(clauseo_acp::Error::Closed))
    }
}

/// Where a steer goes (`target`).
#[derive(Debug, Clone, Copy)]
pub(super) enum Target {
    /// A real steer into this running turn.
    Join(TurnId),
    /// The agent's steer joins a turn, but not now: the message waits as the next turn, behind
    /// this running (or closing) turn.
    Next(TurnId),
    /// The agent has no steer that joins, or nothing runs: a send now (`queue.rs`).
    SendNow,
}

/// Where a steer of `thread` goes. A real steer needs an attached agent whose steer joins the
/// answer (both flags), a prompting turn not cancelled, and no permission or question open. An
/// agent whose steer joins (by its flags, or before its first attach by its kind, as the view's
/// `steer_joins_turn` guesses: Claude's) but cannot take it now gets [`Target::Next`] while a
/// turn runs or closes.
pub(super) fn target(actor: &mut Actor, thread: ThreadId) -> Target {
    let claude = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Claude));
    let Some(state) = state(actor, thread) else {
        return Target::SendNow;
    };
    let agent = state.agent.as_deref();
    let joins = match agent.and_then(|agent| agent.info.as_ref()) {
        Some(info) => info
            .capabilities
            .steering
            .is_some_and(|steering| steering.native && steering.idle_fallback_to_host),
        None => claude,
    };
    let running = state
        .current
        .as_ref()
        .map(|current| current.turn)
        .or_else(|| state.closing.as_ref().map(|closing| closing.close.turn));
    let Some(running) = running.filter(|_| joins) else {
        return Target::SendNow;
    };
    let attached = agent.filter(|agent| agent.stage == Stage::Attached);
    let asking = attached.is_some_and(|agent| {
        !(agent.permissions.is_empty()
            && agent.elicitations.is_empty()
            && agent.url_elicitations.is_empty())
    });
    let now = state.current.as_deref().is_some_and(|current| {
        current.turn == running && current.phase == TurnPhase::Prompting && !current.cancelled
    });
    if now && attached.is_some() && !asking {
        Target::Join(running)
    } else {
        Target::Next(running)
    }
}

/// `SteerResolved { SentNow }` of a steer that could not be real.
pub(super) fn sent_now(message: MessageId, turn: TurnId) -> SteerResolved {
    SteerResolved {
        message,
        outcome: SteerOutcome::SentNow { turn },
        aside: Vec::new(),
    }
}

/// `SteerResolved { Requeued }` of a steer that waits as the next turn (`next`).
pub(super) fn requeued(message: MessageId, turn: TurnId) -> SteerResolved {
    SteerResolved {
        message,
        outcome: SteerOutcome::Requeued { turn },
        aside: Vec::new(),
    }
}

/// A steer that cannot join `running` now: one commit of its `UserMessage { Steer }` with a new
/// turn, that turn's `TurnQueued` behind `running` marked `at_head`, `SteerResolved {
/// Requeued }` and the receipt. It is never sent as a steer: it joins the head, behind any turn
/// already sent now or requeued there, and starts once `running` has ended `EndTurn`; any other
/// stop leaves it not sent (`unsent.rs`).
pub(super) fn next(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    running: TurnId,
    content: Vec<ContentBlock>,
) -> Result<(), Error> {
    super::refuse_full(actor, thread)?;
    tracing::info!(target: LIFECYCLE, %thread, turn = %running, why = why::CANNOT_JOIN_NOW, "a steer was requeued");
    log_queued(
        actor,
        ctx,
        thread,
        Some(running),
        content,
        Joins::Next,
        |turn, message, content, queue| {
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
                    content,
                    forced: true,
                    host_held: true,
                },
            );
            After::Waits(turn)
        },
    )
}

/// One commit of `UserMessage { turn: <running>, delivery: Steer }` and the receipt
/// (`PromptAccepted` with the running turn); the steer is sent once it is durable.
pub(super) fn steer(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    turn: TurnId,
    content: Vec<ContentBlock>,
) -> Result<(), Error> {
    let now = actor.now().wall;
    let message: MessageId = actor.ids.mint(now)?;
    let built = build::user_message(turn, message, None, content.clone(), Delivery::Steer);
    let outcome = CommandOutcome::PromptAccepted { turn, message };
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    out.push(actor, built.value)?;
    out.draft.blobs(built.blobs);
    out.draft
        .receipt(ctx.receipt(Some(StreamId::Thread(thread)), outcome, now));
    let ask = Ask {
        turn,
        message,
        content,
        sent: now,
        places: Vec::new(),
    };
    out.send_tagged(
        actor,
        command_tag(ctx, outcome, thread, After::Steer(Box::new(ask))),
    )
}

/// The steer's message is durable: mark Claude's transcripts, send it to the agent, even if the
/// turn has just ended (the agent then answers `promptRequired`), and arm its wait. With no
/// attached agent it moves to a new turn.
pub(super) fn send(actor: &mut Actor, thread: ThreadId, mut ask: Ask) {
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| {
            let session = agent.session.clone()?;
            Some((agent.id, agent.link.client.clone(), session))
        });
    let Some((id, client, session)) = agent else {
        requeue(actor, thread, ask, why::NO_AGENT);
        return;
    };
    let message = ask.message;
    let content = ask.content.clone();
    tracing::info!(target: LIFECYCLE, %thread, turn = %ask.turn, "a steer was sent");
    ask.places = confirm::mark(actor, thread);
    let timer = Some(arm(actor, thread, message));
    if let Some(queue) = queue(actor, thread) {
        queue.steers.push(Pending { ask, timer });
    }
    let post = actor.post.clone();
    spawn(actor, async move {
        let result = client.steer(&session, content).await;
        let reply = AgentReply::Steer(Box::new(Steered { message, result }));
        let input = TurnInput::Agent {
            thread,
            agent: id,
            reply,
        };
        let _gone = post.try_send(Input::Turn(input));
    });
}

/// The answer's kind, as the lifecycle log names it.
fn kind(result: &Result<SteerResult, clauseo_acp::Error>) -> &'static str {
    match result {
        Ok(SteerResult::Injected) => "injected",
        Ok(SteerResult::PromptRequired) => "prompt_required",
        Ok(SteerResult::StartedNewTurn) => "started_new_turn",
        Ok(SteerResult::Failed) | Err(_) => "failed",
    }
}

/// The agent answered a steer: `Injected` hands it, with the tool calls still running, to its
/// confirmation (`confirm.rs`); any other answer moves it to a new turn at the head. Then the
/// queue may move.
pub(in super::super) fn on_steered(actor: &mut Actor, thread: ThreadId, steered: Steered) {
    let Steered { message, result } = steered;
    let outcome = kind(&result);
    let Some(Pending { ask, timer }) = take_pending(actor, thread, message) else {
        // Settled already: its wait ran out, its turn stopped, or the lawyer sent it.
        tracing::info!(target: LIFECYCLE, %thread, outcome, "a steer was answered after it was settled");
        return;
    };
    if let Some(timer) = timer {
        timer.abort();
    }
    tracing::info!(target: LIFECYCLE, %thread, turn = %ask.turn, outcome, "a steer was answered");
    match result {
        Ok(SteerResult::Injected) => {
            let aside = actor
                .loaded(StreamId::Thread(thread))
                .map(|loaded| loaded.sent.unfinished_tools(ask.turn).collect())
                .unwrap_or_default();
            confirm::accepted(actor, thread, ask, aside);
        }
        Ok(SteerResult::StartedNewTurn) => {
            tracing::warn!(%thread, "the agent started a turn of its own for a steer");
            cancel_agent_turn(actor, thread);
            requeue(actor, thread, ask, why::STARTED_NEW_TURN);
        }
        Ok(SteerResult::PromptRequired) => requeue(actor, thread, ask, why::PROMPT_REQUIRED),
        Ok(SteerResult::Failed) => requeue(actor, thread, ask, why::FAILED),
        Err(error) => {
            tracing::warn!(%thread, error = %Cause(&error), "a steer failed; it goes next");
            requeue(actor, thread, ask, why::FAILED);
        }
    }
    move_on(actor, thread);
    sync_held(actor, thread);
}

/// Arms the wait for the steer of `message`'s answer.
fn arm(actor: &mut Actor, thread: ThreadId, message: MessageId) -> futures::future::AbortHandle {
    actor.arm_timer(
        STEER_WAIT,
        TurnTimer {
            thread,
            kind: TimerKind::Steer(message),
        },
    )
}

/// The end of `turn` is durable. Its steers accepted and unconfirmed get their checks after the
/// end (`confirm.rs`). After `EndTurn` the unanswered ones keep waiting for their answer; after
/// any other stop they are not sent, and a later answer is ignored.
pub(super) fn turn_closed(actor: &mut Actor, thread: ThreadId, turn: TurnId, stop: StopReason) {
    confirm::turn_ended(actor, thread, turn, stop);
    if stop == StopReason::EndTurn {
        return;
    }
    let Some(queue) = queue(actor, thread) else {
        return;
    };
    let mut stopped: Vec<Ask> = queue
        .steers
        .extract_if(.., |pending| pending.ask.turn == turn)
        .map(|pending| {
            if let Some(timer) = pending.timer {
                timer.abort();
            }
            pending.ask
        })
        .collect();
    stopped.sort_by_key(|ask| ask.message);
    for ask in &stopped {
        confirm::not_sent(actor, thread, ask, why::TURN_STOPPED);
    }
}

/// Whether a steer sent into `turn` is not settled yet: unanswered, or accepted and not
/// confirmed.
pub(super) fn unsettled(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    queue(actor, thread)
        .is_some_and(|queue| queue.steers.iter().any(|pending| pending.ask.turn == turn))
        || confirm::waits(actor, thread, turn)
}

/// `SendQueuedNow` naming the turn a steer was sent into: the lawyer sends what the agent was
/// not seen to take in. Its steers unanswered or unconfirmed move now to new turns at the head,
/// which start once the turn has ended, whatever its stop, and a running turn is cancelled.
pub(super) fn send_now(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    let running = state
        .current
        .as_ref()
        .is_some_and(|current| current.turn == turn);
    let mut asks: Vec<Ask> = state
        .queue
        .steers
        .extract_if(.., |pending| pending.ask.turn == turn)
        .map(|pending| {
            if let Some(timer) = pending.timer {
                timer.abort();
            }
            pending.ask
        })
        .collect();
    asks.extend(confirm::take(actor, thread, turn));
    asks.sort_by_key(|ask| ask.message);
    requeue_all(actor, thread, asks, why::SEND_NOW);
    if running {
        cancel::cancel_current(actor, thread, cancel::why::SEND_NOW);
    }
}

/// The steer of `message` got no answer within [`STEER_WAIT`] of its send: while its turn
/// runs it moves to a new turn, as a failed steer does; once its turn has ended it is not sent.
/// A later answer is ignored.
pub(in super::super) fn on_steer_timer(actor: &mut Actor, thread: ThreadId, message: MessageId) {
    let Some(Pending { ask, .. }) = take_pending(actor, thread, message) else {
        return;
    };
    tracing::info!(target: LIFECYCLE, %thread, turn = %ask.turn, outcome = "no_answer_in_30s", "a steer was answered");
    let running = state(actor, thread)
        .and_then(|state| state.current.as_ref())
        .is_some_and(|current| current.turn == ask.turn);
    if running {
        requeue(actor, thread, ask, why::NO_ANSWER);
    } else {
        confirm::not_sent(actor, thread, &ask, why::NO_CONFIRMATION);
        move_on(actor, thread);
        sync_held(actor, thread);
    }
}

/// Takes the steer of `message` out of those waiting for an answer.
fn take_pending(actor: &mut Actor, thread: ThreadId, message: MessageId) -> Option<Pending> {
    let steers = &mut queue(actor, thread)?.steers;
    let at = steers
        .iter()
        .position(|pending| pending.ask.message == message)?;
    Some(steers.remove(at))
}

/// An adapter ignored the opt-in and started a turn the host did not send: cancel it.
fn cancel_agent_turn(actor: &mut Actor, thread: ThreadId) {
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .and_then(|agent| Some((agent.link.client.clone(), agent.session.clone()?)));
    if let Some((client, session)) = agent {
        tracing::info!(target: LIFECYCLE, %thread, why = why::STARTED_NEW_TURN, "a cancel was sent");
        spawn(actor, async move {
            if let Err(error) = client.cancel(&session).await {
                tracing::debug!(%error, "session/cancel after a steer failed");
            }
        });
    }
}
