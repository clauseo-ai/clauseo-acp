//! Agent events: the rate cap per session, then each event by its origin. `Turn` of the live
//! turn is logged; `Late` is dropped, but a session title still counts while its window is open
//! (D-070); `Idle` is the live agent-started turn's, else logged, unless it needs a turn: text
//! and new tool calls then wait for a turn of their own (`own_turn.rs`); `Replay` is dropped
//! when the log already has it and otherwise logged outside a turn, which for text and tool
//! calls (they name a turn) means dropped with a warning.

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole, Origin};
use clauseo_protocol::{
    AgentSessionId, CommandsAvailable, CompactString, CompactionStatus, ConfigOptionsReplaced,
    ContentBlock, DomainEvent, MessageId, ModeChanged, Notice, PlanReplaced, StreamId, TextKind,
    ThreadId, TurnId, UsageUpdated,
};
use clauseo_store::BlobIn;

use super::super::Actor;
use super::agents::{Agent, Stage};
use super::out::Out;
use super::{Then, TimerKind, TurnTimer, asks, current, lost, offline, own_turn, state, text};
use crate::build;
use crate::coalesce::TextIn;
use crate::error::Error;
use crate::log_file::Cause;
use crate::rate::{Admitted, RatedUpdate};

pub(super) mod gave_up;
mod title;
mod tools;
mod warning;

pub(super) use tools::known_tool;
use tools::{tool_call, tool_update};

/// Where an update belongs.
#[derive(Debug, Clone, Copy)]
enum Context {
    /// The live turn.
    Live(TurnId),
    /// No turn: an idle update, or a replayed one.
    Outside { replay: bool },
}

pub(super) fn agent(actor: &mut Actor, thread: ThreadId) -> Option<&mut Agent> {
    state(actor, thread).and_then(|state| state.agent.as_deref_mut())
}

/// One event from `thread`'s agent, through the session's rate cap.
pub(super) fn on_event(actor: &mut Actor, thread: ThreadId, event: Box<AcpEvent>) {
    let now = actor.now().mono;
    let Some(agent) = agent(actor, thread) else {
        tracing::debug!(%thread, "an event from an agent the thread no longer has");
        return;
    };
    // The agent is at work, in a turn or between turns: its idle clock starts again (R20b).
    super::idle::active(&mut agent.idle);
    if agent.stage == Stage::Loading
        && match *event {
            AcpEvent::Update {
                origin: Origin::Replay,
                ..
            } => true,
            AcpEvent::Update { .. } => agent.preparing,
            AcpEvent::PermissionRequested { .. }
            | AcpEvent::ElicitationRequested { .. }
            | AcpEvent::ElicitationCompleted { .. }
            | AcpEvent::Limit { .. }
            | AcpEvent::TooLong { .. }
            | AcpEvent::ConnectionLost { .. }
            | AcpEvent::Warning { .. } => false,
        }
    {
        agent.replayed.push(*event);
        return;
    }
    match agent.rate.admit(now, RatedUpdate(event)) {
        Admitted::Pass(item) => handle(actor, thread, *item.0),
        Admitted::Flush(items) => {
            for item in items {
                handle(actor, thread, *item.0);
            }
        }
        Admitted::Held { arm } => {
            if let Some(at) = arm
                && !agent.rate_armed
            {
                agent.rate_armed = true;
                let timer = TurnTimer {
                    thread,
                    kind: TimerKind::Rate,
                };
                let _handle = actor.arm_timer(at.saturating_sub(now), timer);
            }
        }
    }
}

/// The rate window ended: pass on what it held.
pub(super) fn on_rate_timer(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    agent.rate_armed = false;
    for item in agent.rate.window_end(now) {
        handle(actor, thread, *item.0);
    }
}

/// Passes on everything the rate cap holds, before the turn ends or is cancelled.
pub(super) fn flush_rate(actor: &mut Actor, thread: ThreadId) {
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    if !agent.rate.is_holding() {
        return;
    }
    for item in agent.rate.flush() {
        handle(actor, thread, *item.0);
    }
}

/// One event, past the rate cap. A request with no prompt in flight waits for a turn
/// (`own_turn.rs`).
pub(super) fn handle(actor: &mut Actor, thread: ThreadId, event: AcpEvent) {
    let Some(event) = own_turn::hold_ask(actor, thread, event) else {
        return;
    };
    match event {
        AcpEvent::Update {
            session,
            origin,
            update,
        } => on_update(actor, thread, &session, origin, update),
        event @ AcpEvent::PermissionRequested { .. } => asks::on_permission(actor, thread, event),
        AcpEvent::ElicitationRequested {
            session,
            origin: _,
            key,
            elicitation_id,
            acp_tool_call_id,
            request,
        } => asks::on_elicitation(
            actor,
            thread,
            &session,
            key,
            elicitation_id,
            acp_tool_call_id,
            request,
        ),
        AcpEvent::ElicitationCompleted { elicitation_id } => {
            // The lawyer's answer already logged `ElicitationResolved`; nothing to log.
            if let Some(agent) = agent(actor, thread) {
                agent.url_elicitations.remove(&elicitation_id);
            }
            super::super::limits::asking_changed(actor);
            offline::ask_ended(actor, thread);
        }
        AcpEvent::Limit {
            session,
            origin,
            signal,
        } => asks::on_limit(actor, thread, &session, origin, &signal),
        AcpEvent::TooLong { origin, kind, .. } => {
            // The turn's end reads it with the answer's own (`end.rs`).
            if let Origin::Turn(turn) = origin
                && let Some(current) = current(actor, thread).filter(|current| current.turn == turn)
            {
                current.too_long = current.too_long.or(Some(kind));
            }
        }
        AcpEvent::ConnectionLost { origin, lost, .. } => {
            offline::on_connection_lost(actor, thread, origin, &lost);
        }
        AcpEvent::Warning { session, warning } => warning::log(thread, session.as_ref(), &warning),
    }
}

/// A session title that comes late, after a cancel, still names the thread while its title
/// window is open (D-070): a cancel that lands mid-answer marks the session late, and the agent's
/// title for that answer may come after it. Other late updates stay dropped.
fn late_title_counts(
    actor: &mut Actor,
    thread: ThreadId,
    origin: Origin,
    update: &AgentUpdate,
) -> bool {
    matches!(origin, Origin::Late(_))
        && matches!(update, AgentUpdate::SessionTitle { .. })
        && super::state::title_open(actor, thread, false)
}

/// Where an update of `origin` belongs, or `None` to drop it.
fn context(actor: &mut Actor, thread: ThreadId, origin: Origin) -> Option<Context> {
    match origin {
        Origin::Turn(turn) => {
            super::state::live(actor, thread, turn).then_some(Context::Live(turn))
        }
        Origin::Late(_) => None,
        Origin::Replay => Some(Context::Outside { replay: true }),
        Origin::Idle => Some(
            own_turn::live(actor, thread).map_or(Context::Outside { replay: false }, Context::Live),
        ),
    }
}

fn on_update(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    origin: Origin,
    update: AgentUpdate,
) {
    // A cancelled agent-started turn's output, with no prompt in flight (`own_turn/late.rs`).
    if origin == Origin::Idle && own_turn::late_drops(actor, thread, &update) {
        return;
    }
    let context = match context(actor, thread, origin) {
        Some(context) => context,
        None if late_title_counts(actor, thread, origin, &update) => {
            Context::Outside { replay: false }
        }
        None => return,
    };
    let update = match context {
        Context::Live(turn) => match offline::on_live(actor, thread, turn, session, update) {
            Some(update) => update,
            None => return,
        },
        Context::Outside { .. } if offline::outside_note(thread, &update) => return,
        Context::Outside { replay: false } if own_turn::holds(actor, thread, session, &update) => {
            own_turn::hold(actor, thread, session, update);
            return;
        }
        Context::Outside { .. } => update,
    };
    let ends = own_turn::ends_turn(&update);
    log_update(actor, thread, session, context, update);
    if ends && let Context::Live(turn) = context {
        own_turn::usage_ended(actor, thread, turn);
    }
}

/// A live update of `turn` the offline watch held back, logged now (`offline.rs`).
pub(super) fn log_live(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    session: &AgentSessionId,
    update: AgentUpdate,
) {
    log_update(actor, thread, session, Context::Live(turn), update);
}

fn log_update(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    context: Context,
    update: AgentUpdate,
) {
    let done = match update {
        AgentUpdate::MessageChunk {
            role,
            acp_message_id,
            content,
        } => message_chunk(
            actor,
            thread,
            context,
            session,
            role,
            acp_message_id,
            content,
        ),
        AgentUpdate::ToolCall(call) => tool_call(actor, thread, context, session, call),
        AgentUpdate::ToolCallUpdate { acp_id, patch } => {
            tool_update(actor, thread, context, session, &acp_id, patch)
        }
        update @ (AgentUpdate::Plan(_)
        | AgentUpdate::Commands(_)
        | AgentUpdate::CurrentMode { .. }
        | AgentUpdate::ConfigOptions(_)
        | AgentUpdate::SessionTitle { .. }
        | AgentUpdate::Usage { .. }
        | AgentUpdate::Notice { .. }
        | AgentUpdate::CompactionStarted { .. }
        | AgentUpdate::CompactionEnded { .. }
        | AgentUpdate::CompactionSummaryChunk { .. }) => plain(actor, thread, update, context),
    };
    if let Err(error) = done {
        tracing::warn!(%thread, error = %Cause(&error), "an agent update was not logged");
    }
}

/// A message chunk: text goes to the coalescer, any other block is a `ContentAppended`. User
/// chunks are dropped: the host logs the lawyer's prompts itself.
fn message_chunk(
    actor: &mut Actor,
    thread: ThreadId,
    context: Context,
    session: &AgentSessionId,
    role: MessageRole,
    acp_message_id: Option<CompactString>,
    content: ContentBlock,
) -> Result<(), Error> {
    let kind = match role {
        MessageRole::User => return Ok(()),
        MessageRole::Answer => TextKind::Answer,
        MessageRole::Thought => TextKind::Thought,
    };
    let turn = match context {
        Context::Live(turn) => turn,
        Context::Outside { replay } => {
            let covered = acp_message_id
                .as_deref()
                .is_some_and(|acp| known_message(actor, thread, session, acp).is_some());
            if !covered {
                tracing::warn!(%thread, replay, "agent text outside a turn was dropped");
            }
            return Ok(());
        }
    };
    let message = message_id(actor, thread, session, role, acp_message_id.as_ref())?;
    match content {
        ContentBlock::Text { text } => text::on_delta(
            actor,
            thread,
            TextIn {
                turn,
                message,
                acp_message_id,
                kind,
                text,
            },
        ),
        block @ (ContentBlock::Image { .. }
        | ContentBlock::Audio { .. }
        | ContentBlock::ResourceLink { .. }
        | ContentBlock::Resource { .. }
        | ContentBlock::Unsupported { .. }) => {
            let built = build::content_appended(turn, message, acp_message_id, kind, block);
            // The block belongs to the message, so a chunk without an id after it continues it.
            let run = agent(actor, thread).and_then(|agent| agent.run);
            commit(actor, thread, [built.value.into()], built.blobs)?;
            if let Some(agent) = agent(actor, thread) {
                agent.run = run;
            }
            Ok(())
        }
    }
}

/// The message an ACP message id names in this session, if the host has seen it.
fn known_message(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    acp: &str,
) -> Option<MessageId> {
    let logged = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.sent.message(session, acp));
    let agent = agent(actor, thread)?;
    agent
        .messages
        .get(acp)
        .copied()
        .or(logged)
        .or_else(|| agent.replay.as_ref()?.message(session, acp))
}

/// The message id of a chunk: by its ACP id when it has one, else the run's while the role
/// stays and no other event came between, else a new one.
fn message_id(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    role: MessageRole,
    acp: Option<&CompactString>,
) -> Result<MessageId, Error> {
    let known = match acp {
        Some(acp) => known_message(actor, thread, session, acp),
        None => agent(actor, thread)
            .and_then(|agent| agent.run)
            .and_then(|(run_role, id)| (run_role == role).then_some(id)),
    };
    let id = match known {
        Some(id) => id,
        None => actor.ids.mint(actor.clock.now().wall)?,
    };
    let agent = agent(actor, thread).ok_or_else(|| lost("the agent"))?;
    if let Some(acp) = acp
        && known.is_none()
    {
        agent.messages.insert(acp.clone(), id);
    }
    agent.run = Some((role, id));
    Ok(id)
}

/// An update that needs no turn: logged as its event.
fn plain(
    actor: &mut Actor,
    thread: ThreadId,
    update: AgentUpdate,
    context: Context,
) -> Result<(), Error> {
    let event: DomainEvent = match update {
        AgentUpdate::Plan(entries) => PlanReplaced { entries }.into(),
        AgentUpdate::Commands(commands) => CommandsAvailable {
            commands: super::logout::hide(actor, thread, commands),
        }
        .into(),
        AgentUpdate::CurrentMode { mode_id } => {
            super::forced_mode::reset_if_refused(actor, thread, &mode_id);
            ModeChanged { mode_id }.into()
        }
        AgentUpdate::ConfigOptions(options) => ConfigOptionsReplaced { options }.into(),
        AgentUpdate::Usage { used, size, cost } => UsageUpdated {
            context_used: Some(used),
            context_size: Some(size),
            cost,
        }
        .into(),
        // An agent's notice is its own free text: it never carries a kind.
        AgentUpdate::Notice { level, text } => Notice {
            level,
            text,
            kind: None,
        }
        .into(),
        AgentUpdate::SessionTitle { title } => {
            return title::changed(actor, thread, title, context);
        }
        AgentUpdate::CompactionStarted { acp_id } => {
            if let Some(agent) = agent(actor, thread) {
                agent.compaction.remove(&acp_id);
            }
            let built = build::compaction_changed(CompactionStatus::Started, None);
            return commit(actor, thread, [built.value.into()], built.blobs);
        }
        AgentUpdate::CompactionSummaryChunk { acp_id, content } => {
            if let (Some(agent), ContentBlock::Text { text }) = (agent(actor, thread), content) {
                agent.compaction.entry(acp_id).or_default().push_str(&text);
            }
            return Ok(());
        }
        AgentUpdate::CompactionEnded { acp_id, completed } => {
            let summary = agent(actor, thread).and_then(|agent| agent.compaction.remove(&acp_id));
            if !completed {
                tracing::warn!(%thread, "a compaction failed or was cancelled");
                return Ok(());
            }
            let built = build::compaction_changed(CompactionStatus::Completed, summary);
            return commit(actor, thread, [built.value.into()], built.blobs);
        }
        AgentUpdate::MessageChunk { .. }
        | AgentUpdate::ToolCall(_)
        | AgentUpdate::ToolCallUpdate { .. } => return Err(lost("a turn update")),
    };
    commit(actor, thread, [event], Vec::new())
}

/// Commits non-text events (after the held text); a chunk without an id after them starts a
/// new message.
pub(super) fn commit(
    actor: &mut Actor,
    thread: ThreadId,
    events: impl IntoIterator<Item = DomainEvent>,
    blobs: Vec<BlobIn>,
) -> Result<(), Error> {
    let mut out = Out::new(actor, thread, None)?;
    for event in events {
        out.push(actor, event)?;
    }
    out.draft.blobs(blobs);
    out.send(actor, Then::Nothing)?;
    if let Some(agent) = agent(actor, thread) {
        agent.run = None;
    }
    Ok(())
}
