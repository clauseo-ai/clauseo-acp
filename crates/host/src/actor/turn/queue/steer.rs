//! A real steer: the lawyer's message joins the running turn of an agent whose steer adds to
//! the answer (both of `SteeringCapability`'s flags, claude-agent-acp). The message is logged
//! with the running turn before it is sent; the agent's answer decides where it went:
//! `Injected` stays in the running turn (with the tool calls it set aside), anything else moves
//! it to a new turn at the queue's head (`Requeued`). An answer that does not come within
//! [`STEER_WAIT`] after the steered turn ended counts as `Failed`, so an agent that never answers
//! cannot freeze the queue. The wait starts at the turn's end, not at the send: while the turn
//! runs the agent may answer only once it takes the message in (after a long tool call, say),
//! and a steer timed out then would be requeued and reach the agent twice.
//! Every other steer is a send now.

use clauseo_acp::SteerResult;
use clauseo_protocol::{
    CommandOutcome, ContentBlock, Delivery, MessageId, SteerOutcome, SteerResolved, StreamId,
    ThreadId, TurnId, TurnQueued,
};

use super::super::super::Actor;
use super::super::super::commands::CommandCtx;
use super::super::super::input::Input;
use super::super::agents::Stage;
use super::super::out::Out;
use super::super::{
    AgentReply, Then, TimerKind, TurnInput, TurnPhase, TurnTimer, lost, spawn, state,
};
use super::{After, Queued, command_tag, drop_turn, move_on, queue, sync_held};
use crate::build;
use crate::error::Error;

/// How long a steer waits for the agent's answer, from the end of the turn it was sent into,
/// before it moves to a new turn.
const STEER_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// A steer sent and not answered yet, with its answer's deadline once its turn has ended.
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

/// The running turn a steer can join: its prompt is in flight, it is not cancelled, and its
/// agent's steer adds to the answer.
pub(super) fn target(actor: &mut Actor, thread: ThreadId) -> Option<TurnId> {
    let state = state(actor, thread)?;
    let current = state.current.as_deref()?;
    if current.phase != TurnPhase::Prompting || current.cancelled {
        return None;
    }
    let joins = state
        .agent
        .as_deref()
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| agent.info.as_ref())
        .and_then(|info| info.capabilities.steering)
        .is_some_and(|steering| steering.native && steering.idle_fallback_to_host);
    joins.then_some(current.turn)
}

/// `SteerResolved { SentNow }` of a steer that could not be real.
pub(super) fn sent_now(message: MessageId, turn: TurnId) -> SteerResolved {
    SteerResolved {
        message,
        outcome: SteerOutcome::SentNow { turn },
        aside: Vec::new(),
    }
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
/// agent then answers `promptRequired`). With no attached agent it moves to a new turn.
pub(super) fn send(actor: &mut Actor, thread: ThreadId, ask: Ask) {
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| {
            let session = agent.session.clone()?;
            Some((agent.id, agent.link.client.clone(), session))
        });
    let Some((id, client, session)) = agent else {
        requeue(actor, thread, ask);
        return;
    };
    let message = ask.message;
    let content = ask.content.clone();
    // The wait starts now only if the steered turn has already ended; else at its end.
    let running = state(actor, thread).is_some_and(|state| {
        state
            .current
            .as_ref()
            .is_some_and(|current| current.turn == ask.turn)
            || state
                .closing
                .as_ref()
                .is_some_and(|closing| closing.close.turn == ask.turn)
    });
    let timer = (!running).then(|| arm(actor, thread, message));
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

/// The agent answered a steer: `Injected` logs where it went with the tool calls still
/// running; any other answer moves it to a new turn at the head. Then the queue may move.
pub(in super::super) fn on_steered(actor: &mut Actor, thread: ThreadId, steered: Steered) {
    let Steered { message, result } = steered;
    let Some(Pending { ask, timer }) = take_pending(actor, thread, message) else {
        // It already counted as failed when its wait ran out.
        tracing::debug!(%thread, "a steer answered after its wait ran out");
        return;
    };
    if let Some(timer) = timer {
        timer.abort();
    }
    match result {
        Ok(SteerResult::Injected) => {
            let logged = injected(actor, thread, &ask);
            if let Err(error) = logged {
                tracing::warn!(%thread, %error, "an injected steer was not logged");
            }
        }
        Ok(SteerResult::StartedNewTurn) => {
            tracing::warn!(%thread, "the agent started a turn of its own for a steer");
            cancel_agent_turn(actor, thread);
            requeue(actor, thread, ask);
        }
        Ok(SteerResult::PromptRequired | SteerResult::Failed) => requeue(actor, thread, ask),
        Err(error) => {
            tracing::warn!(%thread, %error, "a steer failed; it goes next");
            requeue(actor, thread, ask);
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

/// The end of `turn` is durable: each steer sent into it and still unanswered starts its wait.
pub(super) fn turn_closed(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let waiting: Vec<MessageId> = queue(actor, thread)
        .map(|queue| {
            queue
                .steers
                .iter()
                .filter(|pending| pending.ask.turn == turn && pending.timer.is_none())
                .map(|pending| pending.ask.message)
                .collect()
        })
        .unwrap_or_default();
    for message in waiting {
        let timer = arm(actor, thread, message);
        if let Some(pending) = queue(actor, thread).and_then(|queue| {
            queue
                .steers
                .iter_mut()
                .find(|pending| pending.ask.message == message)
        }) {
            pending.timer = Some(timer);
        }
    }
}

/// The steer of `message` got no answer within [`STEER_WAIT`] of its turn's end: it moves to a
/// new turn, as a failed steer does, and a later answer is ignored.
pub(in super::super) fn on_steer_timer(actor: &mut Actor, thread: ThreadId, message: MessageId) {
    let Some(Pending { ask, .. }) = take_pending(actor, thread, message) else {
        return;
    };
    tracing::warn!(%thread, "no answer to a steer in {STEER_WAIT:?}; it goes next");
    requeue(actor, thread, ask);
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
        spawn(actor, async move {
            if let Err(error) = client.cancel(&session).await {
                tracing::debug!(%error, "session/cancel after a steer failed");
            }
        });
    }
}

/// The steer moves to a new turn at the queue's head, which starts once the steered turn has
/// ended: one commit of `SteerResolved { Requeued }` and `TurnQueued` behind the steered turn.
fn requeue(actor: &mut Actor, thread: ThreadId, ask: Ask) {
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            if let Err(error) = loaded.and_then(|()| log_requeue(actor, thread, ask)) {
                tracing::warn!(%thread, %error, "a steer was not moved to a new turn");
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
    let waits = TurnQueued::new(thread, turn, thread, Some(ask.turn))
        .map_err(|error| Error::Internal(error.to_string()))?;
    let resolved = SteerResolved {
        message: ask.message,
        outcome: SteerOutcome::Requeued { turn },
        aside: Vec::new(),
    };
    let mut out = Out::new(actor, thread, None)?;
    out.push(actor, resolved)?;
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
