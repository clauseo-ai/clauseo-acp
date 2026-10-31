//! Agent events: the rate cap per session, then each event by its origin. `Turn` of the live
//! turn is logged; `Late` is dropped; `Idle` is logged unless it needs a turn; `Replay` is
//! dropped when the log already has it and otherwise logged outside a turn, which for text and
//! tool calls (they name a turn) means dropped with a warning.

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole, Origin};
use clauseo_protocol::{
    AgentSessionId, CommandsAvailable, CompactString, CompactionStatus, ConfigOptionsReplaced,
    ContentBlock, DomainEvent, MessageId, ModeChanged, Notice, PlanReplaced, RenameSource,
    StreamId, TextKind, ThreadId, ThreadRenamed, Title, ToolCallContent, ToolCallPatch,
    ToolLocation, TurnId, UsageUpdated,
};
use clauseo_store::BlobIn;

use super::super::Actor;
use super::agents::{Agent, Stage};
use super::out::Out;
use super::{Then, TimerKind, TurnTimer, asks, current, lost, state, text};
use crate::build;
use crate::coalesce::TextIn;
use crate::error::Error;
use crate::rate::{Admitted, RatedUpdate};

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
    if agent.stage == Stage::Loading
        && matches!(
            *event,
            AcpEvent::Update {
                origin: Origin::Replay,
                ..
            }
        )
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

fn handle(actor: &mut Actor, thread: ThreadId, event: AcpEvent) {
    match event {
        AcpEvent::Update {
            session,
            origin,
            update,
        } => on_update(actor, thread, &session, origin, update),
        AcpEvent::PermissionRequested {
            session,
            key,
            acp_tool_call_id,
            options,
        } => asks::on_permission(actor, thread, &session, key, &acp_tool_call_id, options),
        AcpEvent::ElicitationRequested {
            session,
            key,
            elicitation_id,
            request,
        } => asks::on_elicitation(actor, thread, &session, key, elicitation_id, request),
        AcpEvent::ElicitationCompleted { elicitation_id } => {
            // The lawyer's answer already logged `ElicitationResolved`; nothing to log.
            if let Some(agent) = agent(actor, thread) {
                agent.url_elicitations.remove(&elicitation_id);
            }
        }
        AcpEvent::Limit {
            session,
            origin,
            signal,
        } => asks::on_limit(actor, thread, &session, origin, &signal),
        AcpEvent::Warning { session, warning } => {
            tracing::warn!(%thread, ?session, ?warning, "the agent sent something unreadable");
        }
    }
}

/// Where an update of `origin` belongs, or `None` to drop it.
fn context(actor: &mut Actor, thread: ThreadId, origin: Origin) -> Option<Context> {
    match origin {
        Origin::Turn(turn) => current(actor, thread)
            .filter(|current| current.turn == turn && current.live())
            .map(|_| Context::Live(turn)),
        Origin::Late(_) => None,
        Origin::Replay => Some(Context::Outside { replay: true }),
        Origin::Idle => Some(Context::Outside { replay: false }),
    }
}

fn on_update(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    origin: Origin,
    update: AgentUpdate,
) {
    let Some(context) = context(actor, thread, origin) else {
        return;
    };
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
        | AgentUpdate::CompactionSummaryChunk { .. }) => plain(actor, thread, update),
    };
    if let Err(error) = done {
        tracing::warn!(%thread, %error, "an agent update was not logged");
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

fn tool_call(
    actor: &mut Actor,
    thread: ThreadId,
    context: Context,
    session: &AgentSessionId,
    call: clauseo_acp::ToolCall,
) -> Result<(), Error> {
    let known = known_tool(actor, thread, session, &call.acp_id);
    let Context::Live(turn) = context else {
        if known.is_none() {
            tracing::warn!(%thread, ?context, "a tool call outside a turn was dropped");
        }
        return Ok(());
    };
    if known.is_some() {
        tracing::debug!(%thread, acp_id = %call.acp_id, "a tool call started twice");
        return Ok(());
    }
    if let Some(current) = current(actor, thread) {
        note_paths(&mut current.tool_paths, &call.locations, &call.content);
    }
    let id = actor.ids.mint(actor.clock.now().wall)?;
    let (built, _fields) = build::tool_call_started(turn, id, call);
    commit(actor, thread, [built.value.into()], built.blobs)
}

fn tool_update(
    actor: &mut Actor,
    thread: ThreadId,
    context: Context,
    session: &AgentSessionId,
    acp_id: &str,
    patch: ToolCallPatch,
) -> Result<(), Error> {
    let found = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.sent.tool_call(session, acp_id))
        .map(|(id, tool)| (id, tool.fields));
    let Some((id, fields)) = found else {
        tracing::debug!(%thread, %acp_id, "an update for an unknown tool call was dropped");
        return Ok(());
    };
    match context {
        Context::Outside { replay: true } => return Ok(()),
        Context::Live(_) => {
            if let Some(current) = current(actor, thread) {
                let locations = patch.locations.as_deref().unwrap_or_default();
                let content = patch.content.as_deref().unwrap_or_default();
                note_paths(&mut current.tool_paths, locations, content);
            }
        }
        Context::Outside { replay: false } => {}
    }
    let (built, _fields) = build::tool_call_updated(id, patch, fields);
    commit(actor, thread, [built.value.into()], built.blobs)
}

/// The tool call ACP knows as `acp_id` in `session`, if the host has it.
pub(super) fn known_tool(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    acp_id: &str,
) -> Option<clauseo_protocol::ToolCallId> {
    let logged = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.sent.tool_call(session, acp_id))
        .map(|(id, _)| id);
    logged.or_else(|| {
        agent(actor, thread)?
            .replay
            .as_ref()?
            .tool_call(session, acp_id)
            .map(|(id, _)| id)
    })
}

/// Adds the matter paths a tool call names (its in-matter locations and diffs).
pub(super) fn note_paths(
    paths: &mut std::collections::BTreeSet<clauseo_protocol::MatterPath>,
    locations: &[ToolLocation],
    content: &[ToolCallContent],
) {
    let diffs = content.iter().filter_map(|item| match item {
        ToolCallContent::Diff { path, .. } => Some(path),
        ToolCallContent::Text { .. }
        | ToolCallContent::Content { .. }
        | ToolCallContent::Unknown { .. } => None,
    });
    for location in locations.iter().chain(diffs) {
        match location {
            ToolLocation::Inside { path, .. } => {
                if !paths.contains(path) {
                    paths.insert(path.clone());
                }
            }
            ToolLocation::Outside { .. } | ToolLocation::Unknown { .. } => {}
        }
    }
}

/// An update that needs no turn: logged as its event.
fn plain(actor: &mut Actor, thread: ThreadId, update: AgentUpdate) -> Result<(), Error> {
    let event: DomainEvent = match update {
        AgentUpdate::Plan(entries) => PlanReplaced { entries }.into(),
        AgentUpdate::Commands(commands) => CommandsAvailable { commands }.into(),
        AgentUpdate::CurrentMode { mode_id } => ModeChanged { mode_id }.into(),
        AgentUpdate::ConfigOptions(options) => ConfigOptionsReplaced { options }.into(),
        AgentUpdate::Usage { used, size, cost } => UsageUpdated {
            context_used: Some(used),
            context_size: Some(size),
            cost,
        }
        .into(),
        AgentUpdate::Notice { level, text } => Notice { level, text }.into(),
        AgentUpdate::SessionTitle { title } => return title_changed(actor, thread, title),
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
                tracing::warn!(%thread, %acp_id, "a compaction failed or was cancelled");
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

/// A session title: `ThreadRenamed { by: Agent }` and the row, when it is a valid title.
fn title_changed(actor: &mut Actor, thread: ThreadId, title: Option<String>) -> Result<(), Error> {
    let Some(title) = title else {
        tracing::debug!(%thread, "the agent cleared its session title");
        return Ok(());
    };
    let title = match Title::new(&title) {
        Ok(title) => title,
        Err(error) => {
            tracing::debug!(%thread, %error, "the agent's session title is not a valid title");
            return Ok(());
        }
    };
    let mut out = Out::new(actor, thread, None)?;
    out.push(
        actor,
        ThreadRenamed {
            title: CompactString::from(title.as_str()),
            by: RenameSource::Agent,
        },
    )?;
    let now = actor.now().wall;
    let entry = actor
        .threads
        .get_mut(&thread)
        .ok_or_else(|| lost("a thread"))?;
    entry.row.title = Some(title.as_str().to_owned());
    entry.row.updated = now;
    out.draft.thread_row(entry.row.clone());
    out.send(actor, Then::Nothing)?;
    if let Some(agent) = agent(actor, thread) {
        agent.run = None;
    }
    Ok(())
}

/// Commits non-text events (after the held text); a chunk without an id after them starts a
/// new message.
pub(super) fn commit<const N: usize>(
    actor: &mut Actor,
    thread: ThreadId,
    events: [DomainEvent; N],
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
