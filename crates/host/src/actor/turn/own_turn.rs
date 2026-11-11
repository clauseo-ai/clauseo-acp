//! An agent-started turn. claude-agent-acp starts a turn of its own, with no `session/prompt`,
//! when a command it put in the background finishes (a steer during a Bash call makes Claude
//! Code background the command and end the prompt). Its updates come with no prompt in flight
//! (`Origin::Idle`), and agent output outside a turn is never dropped: text and tool calls name a
//! turn, so the host opens one for them on the same thread. It has no `UserMessage`; otherwise it
//! is a turn like any other: the matter's writer, the before checkpoint (with the outside edits
//! since the last turn, as for a prompt), `TurnStarted`, live updates, and steps 6 to 8 with the
//! after checkpoint, so the view shows it as an exchange with no question and Undo has its step.
//!
//! A permission request or an elicitation with no live turn opens it too, and waits with the
//! updates: its card shows once the turn is live, never cancelled as outside a turn. Until it is
//! live (and while another turn of the thread starts or ends) these wait here in order, then are
//! logged as the turn's own: the agent-started turn's once it starts, or the
//! turn a prompt starts meanwhile, which they then come first in. Queued prompts wait behind it
//! as behind any turn. A cancel (Stop, Send now), in any phase, sends `session/cancel`, drops
//! the output waiting for it and ends it `Cancelled` (at once once live, as no prompt answers
//! it), and opens its late window (`late.rs`): what the agent sends until it has stopped is
//! dropped, no agent-started turn opens meanwhile, and a prompt sent now waits for it.
//!
//! Its end: the agent's turn ends in the SDK's `result`, which claude-agent-acp sends as a
//! `usage_update` with a cost (a prompted turn's last update, just before its response), so a
//! `Usage` with a cost ends it `EndTurn`. Should that never come, [`QUIET`] with no update ends
//! it, unless a tool call of the turn or a request is open, which it waits for up to [`OPEN_CAP`]
//! of silence: it never runs forever. Agent output while a turn is cancelled, waiting when it
//! ends, within [`CANCEL_WAIT`] of its end, or in an agent-started turn's late window is that
//! turn's tail and is dropped, as `Origin::Late` is.

mod late;

use std::time::Duration;

use clauseo_acp::{AcpEvent, AgentUpdate, Origin};
use clauseo_protocol::{AgentSessionId, ExpiryReason, StopReason, StreamId, ThreadId, TurnId};
use futures::future::AbortHandle;

use super::super::Actor;
use super::agents::Stage;
use super::{
    CANCEL_WAIT, Current, End, TimerKind, TurnPhase, TurnTimer, current, end, host_stopping, state,
    steps, updates,
};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

/// How long an agent-started turn with nothing open waits for an update before it ends. Claude
/// streams its thinking and its text, so a turn goes this long without an update only while a
/// tool runs, which keeps its call open.
pub(super) const QUIET: Duration = Duration::from_secs(60);
/// The longest silence an agent-started turn waits through while a tool call or request of it
/// is open.
pub(super) const OPEN_CAP: Duration = Duration::from_secs(600);

/// Agent output that came outside any turn, waiting for one to hold it.
#[derive(Debug, Default)]
pub(crate) struct Held {
    /// Updates (as `Idle`), permission requests and elicitations, in the order they came.
    updates: Vec<AcpEvent>,
    /// When the thread's last cancelled turn ended, on the monotonic clock.
    cancelled_at: Option<Duration>,
    /// The late window of a cancelled agent-started turn, while it is open.
    late: Option<late::Late>,
}

#[cfg(test)]
pub(super) use late::{QUIET as LATE_QUIET, dropped as late_dropped};
pub(super) use late::{holds_starts, on_timer as on_late_timer};

/// How many updates and requests of `thread` wait for a turn.
#[cfg(test)]
pub(super) fn waiting(actor: &mut Actor, thread: ThreadId) -> usize {
    state(actor, thread).map_or(0, |state| state.own_held.updates.len())
}

/// Whether an `Idle` update of `thread` is dropped by the open late window (`late.rs`).
pub(super) fn late_drops(actor: &mut Actor, thread: ThreadId, update: &AgentUpdate) -> bool {
    late::drops(actor, thread, update)
}

/// What an agent-started current turn keeps.
#[derive(Debug)]
pub(super) struct Own {
    /// The quiet timer, re-armed by each update.
    quiet: Option<AbortHandle>,
    /// The last update, on the monotonic clock.
    last: Duration,
}

/// The agent-started turn of `thread` that is live (`Current::live`), if any: an `Idle` update
/// belongs to it, and moves its quiet timer on.
pub(super) fn live(actor: &mut Actor, thread: ThreadId) -> Option<TurnId> {
    let now = actor.now().mono;
    let current = current(actor, thread).filter(|current| current.live())?;
    let own = current.own.as_mut()?;
    own.last = now;
    let turn = current.turn;
    arm(actor, thread, turn);
    Some(turn)
}

/// Whether an `Idle` update of `thread` waits for a turn: it is answer or thought text, or a
/// tool call the host does not have, or updates wait already (so they keep their order).
pub(super) fn holds(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    update: &AgentUpdate,
) -> bool {
    let waiting = state(actor, thread).is_some_and(|state| !state.own_held.updates.is_empty());
    waiting
        || match update {
            AgentUpdate::MessageChunk { role, .. } => *role != clauseo_acp::MessageRole::User,
            AgentUpdate::ToolCall(call) => {
                updates::known_tool(actor, thread, session, &call.acp_id).is_none()
            }
            AgentUpdate::ToolCallUpdate { .. }
            | AgentUpdate::Plan(_)
            | AgentUpdate::Commands(_)
            | AgentUpdate::CurrentMode { .. }
            | AgentUpdate::ConfigOptions(_)
            | AgentUpdate::SessionTitle { .. }
            | AgentUpdate::Usage { .. }
            | AgentUpdate::Notice { .. }
            | AgentUpdate::CompactionStarted { .. }
            | AgentUpdate::CompactionEnded { .. }
            | AgentUpdate::CompactionSummaryChunk { .. } => false,
        }
}

/// An `Idle` update waits for a turn, which opens now when the thread has none.
pub(super) fn hold(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    update: AgentUpdate,
) {
    let event = AcpEvent::Update {
        session: session.clone(),
        origin: Origin::Idle,
        update,
    };
    if !hold_event(actor, thread, event) {
        tracing::debug!(%thread, "agent output after a cancelled turn was dropped");
    }
}

/// A permission request or an elicitation that came with no prompt in flight (`Idle`) waits for a
/// turn, unless the agent-started turn is live (it is that turn's) or it is a cancelled turn's
/// tail: then it is handed back, for `asks.rs` to ask it, or answer it cancelled. A prompted turn
/// the host still has live has had its prompt answered: the request is the agent's own turn's.
/// Any other event is handed back at once.
pub(super) fn hold_ask(actor: &mut Actor, thread: ThreadId, event: AcpEvent) -> Option<AcpEvent> {
    let idle = matches!(
        event,
        AcpEvent::PermissionRequested {
            origin: Origin::Idle,
            ..
        } | AcpEvent::ElicitationRequested {
            origin: Origin::Idle,
            ..
        }
    );
    let own_live =
        current(actor, thread).is_some_and(|current| current.live() && current.own.is_some());
    if !idle || own_live || tail(actor, thread) {
        if idle {
            late::note(actor, thread);
        }
        return Some(event);
    }
    hold_event(actor, thread, event);
    None
}

/// Whether agent output now is a cancelled turn's tail: the current turn is cancelled, the last
/// one ended cancelled within [`CANCEL_WAIT`], or an agent-started turn's late window is open.
fn tail(actor: &mut Actor, thread: ThreadId) -> bool {
    let now = actor.now().mono;
    state(actor, thread).is_some_and(|state| {
        state.own_held.late.is_some()
            || state
                .current
                .as_ref()
                .is_some_and(|current| current.cancelled)
            || state
                .own_held
                .cancelled_at
                .is_some_and(|at| now.saturating_sub(at) < CANCEL_WAIT)
    })
}

/// Keeps `event` until a turn holds it, and opens one when the thread has none; false, keeping
/// nothing, for a cancelled turn's tail.
fn hold_event(actor: &mut Actor, thread: ThreadId, event: AcpEvent) -> bool {
    if tail(actor, thread) {
        return false;
    }
    let Some(state) = state(actor, thread) else {
        return false;
    };
    let held = &mut state.own_held;
    if held.updates.is_empty() {
        tracing::info!(target: LIFECYCLE, %thread, "agent output outside a turn waits for a turn");
    }
    held.updates.push(event);
    open_if_idle(actor, thread);
    true
}

/// Opens an agent-started turn for the waiting updates when the thread has no turn (none
/// current, none closing), no late window is open and its agent is attached: step 3 (the
/// writer) at once, as its agent runs. Nothing opens while the host stops; the updates then go
/// with the agent.
pub(super) fn open_if_idle(actor: &mut Actor, thread: ThreadId) {
    if host_stopping(actor) || late::is_open(actor, thread) {
        return;
    }
    let Some(state) = state(actor, thread) else {
        return;
    };
    let attached = state
        .agent
        .as_deref()
        .is_some_and(|agent| agent.stage == Stage::Attached);
    if state.own_held.updates.is_empty()
        || state.current.is_some()
        || state.closing.is_some()
        || state.undo.is_some()
        || !attached
    {
        return;
    }
    if let Err(error) = open(actor, thread) {
        tracing::warn!(%thread, error = %Cause(&error), "an agent-started turn did not open");
    }
}

fn open(actor: &mut Actor, thread: ThreadId) -> Result<(), Error> {
    let now = actor.now();
    let turn: TurnId = actor.ids.mint(now.wall)?;
    // Never logged: the turn has no message of the lawyer's.
    let message = actor.ids.mint(now.wall)?;
    let mut current = Current::new(turn, message, Vec::new(), now.mono);
    current.own = Some(Own {
        quiet: None,
        last: now.mono,
    });
    current.phase = TurnPhase::Launching;
    state(actor, thread)
        .ok_or_else(|| super::lost("a thread"))?
        .current = Some(Box::new(current));
    tracing::info!(target: LIFECYCLE, %thread, %turn, "an agent-started turn opens");
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            match loaded {
                Ok(()) => steps::on_launched(actor, thread, turn),
                Err(error) => {
                    tracing::warn!(%thread, error = %Cause(&error), "an agent-started turn did not open");
                    if let Some(state) = state(actor, thread) {
                        state.current.take_if(|current| current.turn == turn);
                    }
                }
            }
            actor.release(stream);
        }),
    );
    Ok(())
}

/// Whether `thread`'s current turn is agent-started.
pub(super) fn is_own(actor: &mut Actor, thread: ThreadId) -> bool {
    current(actor, thread).is_some_and(|current| current.own.is_some())
}

/// The `TurnStarted` of an agent-started turn is durable: it is live, the waiting updates are
/// logged as its own, and its quiet timer runs.
pub(super) fn started(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let now = actor.now().mono;
    let Some(current) = current(actor, thread).filter(|current| current.turn == turn) else {
        return;
    };
    current.phase = TurnPhase::Prompting;
    if let Some(own) = current.own.as_mut() {
        own.last = now;
    }
    tracing::info!(target: LIFECYCLE, %thread, %turn, "an agent-started turn started");
    arm(actor, thread, turn);
    flush(actor, thread, turn);
}

/// A turn of `thread` is live: what waits for a turn is logged as `turn`'s own, its requests
/// asked as the live turn's.
pub(super) fn flush(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let waiting = state(actor, thread)
        .map(|state| std::mem::take(&mut state.own_held.updates))
        .unwrap_or_default();
    for event in waiting {
        match event {
            AcpEvent::Update {
                session, update, ..
            } => {
                let ends = ends_turn(&update);
                updates::log_live(actor, thread, turn, &session, update);
                if ends {
                    usage_ended(actor, thread, turn);
                }
            }
            event @ (AcpEvent::PermissionRequested { .. }
            | AcpEvent::ElicitationRequested { .. }
            | AcpEvent::ElicitationCompleted { .. }
            | AcpEvent::Limit { .. }
            | AcpEvent::Warning { .. }) => updates::handle(actor, thread, event),
        }
    }
}

/// Whether `update` is the agent's end of a turn: a usage update with a cost, which
/// claude-agent-acp sends from the SDK's `result`.
pub(super) fn ends_turn(update: &AgentUpdate) -> bool {
    matches!(update, AgentUpdate::Usage { cost: Some(_), .. })
}

/// A live update of `turn` said the agent's turn ended: an agent-started turn ends `EndTurn`.
pub(super) fn usage_ended(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let own = current(actor, thread)
        .is_some_and(|current| current.turn == turn && current.live() && current.own.is_some());
    if own {
        tracing::info!(target: LIFECYCLE, %thread, %turn, by = "usage", "an agent-started turn ended");
        end_now(actor, thread);
    }
}

fn end_now(actor: &mut Actor, thread: ThreadId) {
    disarm(actor, thread);
    let end = End {
        stop: StopReason::EndTurn,
        failure: None,
        expiry: ExpiryReason::TurnEnded,
        detach: None,
    };
    end::end_turn(actor, thread, end);
}

fn arm(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let timer = actor.arm_timer(
        QUIET,
        TurnTimer {
            thread,
            kind: TimerKind::OwnQuiet(turn),
        },
    );
    if let Some(own) = current(actor, thread).and_then(|current| current.own.as_mut())
        && let Some(old) = own.quiet.replace(timer)
    {
        old.abort();
    }
}

fn disarm(actor: &mut Actor, thread: ThreadId) {
    if let Some(timer) = current(actor, thread)
        .and_then(|current| current.own.as_mut())
        .and_then(|own| own.quiet.take())
    {
        timer.abort();
    }
}

/// [`QUIET`] passed with no update of the agent-started turn `turn`: it ends, unless a tool call
/// or request of it is open and the silence is shorter than [`OPEN_CAP`].
pub(super) fn on_quiet(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let now = actor.now().mono;
    let Some(last) = current(actor, thread)
        .filter(|current| current.turn == turn && current.live())
        .and_then(|current| current.own.as_ref())
        .map(|own| own.last)
    else {
        return;
    };
    let tools = actor
        .loaded(StreamId::Thread(thread))
        .is_some_and(|loaded| loaded.sent.unfinished_tools(turn).next().is_some());
    let asks = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .is_some_and(|agent| {
            !(agent.permissions.is_empty()
                && agent.elicitations.is_empty()
                && agent.url_elicitations.is_empty())
        });
    if (tools || asks) && now.saturating_sub(last) < OPEN_CAP {
        arm(actor, thread, turn);
        return;
    }
    tracing::info!(target: LIFECYCLE, %thread, %turn, by = "quiet", open = tools || asks, "an agent-started turn ended");
    end_now(actor, thread);
}

/// A cancel of the agent-started current turn, its `session/cancel` sent: its late window
/// opens, and it ends `Cancelled` now, as no prompt answers it. Says whether the turn was one.
pub(super) fn cancelled(actor: &mut Actor, thread: ThreadId) -> bool {
    if !is_own(actor, thread) {
        return false;
    }
    late::open(actor, thread);
    disarm(actor, thread);
    end::end_turn(actor, thread, End::cancelled(None));
    true
}

/// A cancel of an agent-started current turn not live yet (launching, waiting for the writer,
/// checkpointing or starting; the agent runs it already): its late window opens, dropping the
/// output waiting for it, and the agent gets `session/cancel`. Any other turn is left to the
/// caller.
pub(super) fn cancel_agent(actor: &mut Actor, thread: ThreadId, why: &'static str) {
    let starting = current(actor, thread)
        .filter(|current| current.own.is_some() && !current.cancelled)
        .is_some_and(|current| {
            matches!(
                current.phase,
                TurnPhase::Accepting
                    | TurnPhase::Launching
                    | TurnPhase::Queued
                    | TurnPhase::Checkpointing
                    | TurnPhase::Starting
            )
        });
    if !starting {
        return;
    }
    late::open(actor, thread);
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| Some((agent.link.client.clone(), agent.session.clone()?)));
    if let Some((client, session)) = agent {
        tracing::info!(target: LIFECYCLE, %thread, why, "a cancel was sent");
        super::spawn(actor, async move {
            if let Err(error) = client.cancel(&session).await {
                tracing::debug!(%error, "session/cancel of an agent-started turn failed");
            }
        });
    }
}

/// A turn of `thread` ends with `stop`: after a cancel, the agent output that waits, and what
/// follows soon, is that turn's tail.
pub(super) fn turn_ending(actor: &mut Actor, thread: ThreadId, stop: StopReason) {
    let now = actor.now().mono;
    disarm(actor, thread);
    if let Some(state) = state(actor, thread) {
        let cancelled = stop == StopReason::Cancelled;
        state.own_held.cancelled_at = cancelled.then_some(now);
        if cancelled && !state.own_held.updates.is_empty() {
            state.own_held.updates.clear();
            tracing::debug!(%thread, "agent output of a cancelled turn was dropped");
        }
    }
}

/// Answers the session's pending permission requests `cancelled`, outside the live turn (a
/// cancelled turn's tail, `asks.rs`).
pub(super) fn cancel_outside_turn(actor: &mut Actor, thread: ThreadId, session: &AgentSessionId) {
    if let Some(agent) = updates::agent(actor, thread) {
        let client = agent.link.client.clone();
        let session = session.clone();
        super::spawn(actor, async move {
            // `cancel` answers every pending permission of the session `cancelled`.
            if let Err(error) = client.cancel(&session).await {
                tracing::debug!(%error, "a permission outside a turn was not cancelled");
            }
        });
    }
}
