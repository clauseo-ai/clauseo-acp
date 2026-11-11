//! A real steer: the lawyer's message joins the running turn of an agent whose steer adds to
//! the answer (both of `SteeringCapability`'s flags, claude-agent-acp), only while the agent can
//! take it now: the turn is prompting, not cancelled, and no permission or question is open
//! (claude-agent-acp defers a steer while one is, and a cancel can then drop it). The message is
//! logged with the running turn before it is sent; the agent's answer decides where it went:
//! `Injected` stays in the running turn (with the tool calls it set aside), anything else moves
//! it to a new turn at the queue's head (`Requeued`). An answer that does not come within
//! [`STEER_WAIT`] of the send, the turn running or not, counts as `Failed`, so an agent that
//! never answers cannot freeze the queue or leave the message unsent; should the agent take it in
//! later all the same, it reaches the agent twice, which is better than not at all. A steer the
//! agent took in goes next all the same when its turn then ends `Cancelled` or `Failed` (a
//! cancel can drop a message the agent had not used yet), and so does one still unanswered then:
//! the turn's end moves them (`turn_closed`). Every steer is logged at info on the lifecycle
//! target as it is sent, answered and requeued, with no text. A steer this agent's steer would
//! join but that cannot join now waits as the next turn ([`next`]); every other steer is a send
//! now.

use clauseo_acp::SteerResult;
use clauseo_protocol::{
    AgentKind, CommandOutcome, ContentBlock, Delivery, MessageId, SteerOutcome, SteerResolved,
    StopReason, StreamId, ThreadId, TurnId, TurnQueued,
};

use super::super::super::Actor;
use super::super::super::commands::CommandCtx;
use super::super::super::input::Input;
use super::super::agents::Stage;
use super::super::out::Out;
use super::super::{
    AgentReply, Then, TimerKind, TurnInput, TurnPhase, TurnTimer, cancel, lost, spawn, state,
};
use super::queued::{Joins, log_queued};
use super::{After, Queued, command_tag, drop_turn, move_on, queue, sync_held};
use crate::build;
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

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
    /// Its turn ended `Cancelled` or `Failed` before the steer was settled.
    pub(in super::super) const TURN_STOPPED: &str = "turn_stopped";
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
    turn: TurnId,
    message: MessageId,
    content: Vec<ContentBlock>,
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
/// already sent now or requeued there, and starts once `running` has ended, whatever its stop.
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
    };
    out.send_tagged(
        actor,
        command_tag(ctx, outcome, thread, After::Steer(Box::new(ask))),
    )
}

/// The steer's message is durable: send it to the agent, even if the turn has just ended (the
/// agent then answers `promptRequired`), and arm its wait. With no attached agent it moves to a
/// new turn.
pub(super) fn send(actor: &mut Actor, thread: ThreadId, ask: Ask) {
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

/// The agent answered a steer: `Injected` logs where it went with the tool calls still
/// running, and is kept until its turn ends in case that turn is cancelled; any other answer
/// moves it to a new turn at the head. Then the queue may move.
pub(in super::super) fn on_steered(actor: &mut Actor, thread: ThreadId, steered: Steered) {
    let Steered { message, result } = steered;
    let outcome = kind(&result);
    let Some(Pending { ask, timer }) = take_pending(actor, thread, message) else {
        // It already went next: its wait ran out, or its turn stopped, or the lawyer sent it.
        tracing::info!(target: LIFECYCLE, %thread, outcome, "a steer was answered after it went next");
        return;
    };
    if let Some(timer) = timer {
        timer.abort();
    }
    tracing::info!(target: LIFECYCLE, %thread, turn = %ask.turn, outcome, "a steer was answered");
    match result {
        Ok(SteerResult::Injected) => {
            let logged = injected(actor, thread, &ask);
            if let Err(error) = logged {
                tracing::warn!(%thread, error = %Cause(&error), "an injected steer was not logged");
            }
            keep_injected(actor, thread, ask);
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

/// An injected steer of the current turn is kept until that turn ends (`turn_closed`); one
/// whose turn has already ended stays where the agent put it.
fn keep_injected(actor: &mut Actor, thread: ThreadId, ask: Ask) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    let running = state
        .current
        .as_ref()
        .is_some_and(|current| current.turn == ask.turn);
    if running {
        state.queue.injected.push(ask);
    }
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

/// The end of `turn` is durable. After `Cancelled` or `Failed`, the steers sent into it that the
/// agent may have lost (taken in, or not answered yet) are taken out, in the order they were
/// sent, for the caller to move to new turns (`requeue_all`) once the queue has moved; a later
/// answer is then ignored. After any other stop the injected ones stay where they went and the
/// unanswered ones keep waiting for their answer.
pub(super) fn turn_closed(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    stop: StopReason,
) -> Vec<Ask> {
    let Some(queue) = queue(actor, thread) else {
        return Vec::new();
    };
    let mut lost: Vec<Ask> = queue
        .injected
        .extract_if(.., |ask| ask.turn == turn)
        .collect();
    if !matches!(stop, StopReason::Cancelled | StopReason::Failed) {
        return Vec::new();
    }
    for pending in queue
        .steers
        .extract_if(.., |pending| pending.ask.turn == turn)
    {
        if let Some(timer) = pending.timer {
            timer.abort();
        }
        lost.push(pending.ask);
    }
    lost.sort_by_key(|ask| ask.message);
    lost
}

/// `asks`, in the order they were sent, each move to a new turn at the head, the first sent
/// first.
pub(super) fn requeue_all(actor: &mut Actor, thread: ThreadId, asks: Vec<Ask>, why: &'static str) {
    for ask in asks.into_iter().rev() {
        requeue(actor, thread, ask, why);
    }
}

/// Whether a steer sent into `turn` is not settled yet: unanswered, or taken into a turn still
/// running.
pub(super) fn unsettled(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    queue(actor, thread).is_some_and(|queue| {
        queue.steers.iter().any(|pending| pending.ask.turn == turn)
            || queue.injected.iter().any(|ask| ask.turn == turn)
    })
}

/// `SendQueuedNow` naming the turn a steer was sent into: a running turn is cancelled, and its
/// steers go next as it ends (`turn_closed`); a turn already ended has its steers still
/// unanswered go next now.
pub(super) fn send_now(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    let running = state
        .current
        .as_ref()
        .is_some_and(|current| current.turn == turn);
    if running {
        cancel::cancel_current(actor, thread, cancel::why::SEND_NOW);
        return;
    }
    let mut asks = Vec::new();
    for pending in state
        .queue
        .steers
        .extract_if(.., |pending| pending.ask.turn == turn)
    {
        if let Some(timer) = pending.timer {
            timer.abort();
        }
        asks.push(pending.ask);
    }
    asks.sort_by_key(|ask| ask.message);
    requeue_all(actor, thread, asks, why::SEND_NOW);
}

/// The steer of `message` got no answer within [`STEER_WAIT`] of its send: it moves to a new
/// turn, as a failed steer does, and a later answer is ignored.
pub(in super::super) fn on_steer_timer(actor: &mut Actor, thread: ThreadId, message: MessageId) {
    let Some(Pending { ask, .. }) = take_pending(actor, thread, message) else {
        return;
    };
    tracing::info!(target: LIFECYCLE, %thread, turn = %ask.turn, outcome = "no_answer_in_30s", "a steer was answered");
    requeue(actor, thread, ask, why::NO_ANSWER);
}

/// Takes the steer of `message` out of those waiting for an answer.
fn take_pending(actor: &mut Actor, thread: ThreadId, message: MessageId) -> Option<Pending> {
    let steers = &mut queue(actor, thread)?.steers;
    let at = steers
        .iter()
        .position(|pending| pending.ask.message == message)?;
    Some(steers.remove(at))
}

/// `SteerResolved { Injected }`, with the running turn's tool calls not finished.
fn injected(actor: &mut Actor, thread: ThreadId, ask: &Ask) -> Result<(), Error> {
    let aside = actor
        .loaded(StreamId::Thread(thread))
        .map(|loaded| loaded.sent.unfinished_tools(ask.turn).collect())
        .unwrap_or_default();
    let resolved = SteerResolved {
        message: ask.message,
        outcome: SteerOutcome::Injected { turn: ask.turn },
        aside,
    };
    let mut out = Out::new(actor, thread, None)?;
    out.push(actor, resolved)?;
    out.send(actor, Then::Nothing)
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

/// The steer moves to a new turn at the queue's head, which starts once the steered turn has
/// ended: one commit of `SteerResolved { Requeued }` and `TurnQueued` behind the steered turn,
/// marked `at_head` so a restart keeps it at the head too (`restore.rs`). An injected steer's
/// `Requeued` comes after its `Injected`, which the view replaces.
fn requeue(actor: &mut Actor, thread: ThreadId, ask: Ask, why: &'static str) {
    tracing::info!(target: LIFECYCLE, %thread, turn = %ask.turn, why, "a steer was requeued");
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            if let Err(error) = loaded.and_then(|()| log_requeue(actor, thread, ask)) {
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

fn log_requeue(actor: &mut Actor, thread: ThreadId, ask: Ask) -> Result<(), Error> {
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
        });
    let sent = out.send(actor, Then::Queue(After::Requeued(turn)));
    if sent.is_err() {
        drop_turn(actor, thread, turn);
    }
    sent
}
