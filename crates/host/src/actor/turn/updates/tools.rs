//! Tool calls and their updates: a live turn's are logged with what they add to the thread's
//! sources; outside a turn a new one is dropped and an update of a known one logged as is.

use clauseo_protocol::{AgentId, AgentSessionId, DomainEvent, StreamId, ThreadId, ToolCallPatch};

use super::super::super::Actor;
use super::super::{current, state};
use super::{Context, agent, commit};
use crate::build;
use crate::error::Error;
use crate::sources::Sources;

pub(super) fn tool_call(
    actor: &mut Actor,
    thread: ThreadId,
    context: Context,
    session: &AgentSessionId,
    mut call: clauseo_acp::ToolCall,
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
        // The turn's paths stay in memory, so they take the call as the agent sent it; what is
        // logged (the sources and the build) takes it with the withheld values scrubbed.
        let fields = crate::sent::CallFields::of_call(&call);
        current.tool_paths.note(&call.acp_id, &fields);
        current.withheld.scrub_call(&mut call);
    }
    let now = actor.clock.now();
    let id = actor.ids.mint(now.wall)?;
    let case_law = source_of(actor, thread, |sources, agent| {
        sources.codex_mcp_started(agent, &call);
        sources.case_law.started(now.mono, turn, id, agent, &call)
    });
    let source = if case_law.is_some() {
        None
    } else {
        source_of(actor, thread, |sources, agent| {
            sources.started(turn, id, agent, &call)
        })
    };
    let (built, _fields) = build::tool_call_started(turn, id, call);
    commit(actor, thread, [built.value.into()], built.blobs)?;
    sourced(
        actor,
        thread,
        source.into_iter().chain(case_law.into_iter().flatten()),
    )
}

/// What a tool call adds to the thread's sources (`sources.rs`), asked of the thread's state
/// with the thread's agent.
fn source_of<T>(
    actor: &mut Actor,
    thread: ThreadId,
    record: impl FnOnce(&mut Sources, &AgentId) -> Option<T>,
) -> Option<T> {
    let agent = actor.check_thread(thread).ok()?.row.agent.clone();
    record(&mut state(actor, thread)?.sources, &agent)
}

/// Logs what a tool call added to the thread's sources, after the call's own event.
fn sourced(
    actor: &mut Actor,
    thread: ThreadId,
    source: impl IntoIterator<Item = DomainEvent>,
) -> Result<(), Error> {
    let events: Vec<DomainEvent> = source.into_iter().collect();
    if events.is_empty() {
        return Ok(());
    }
    let hint = events.iter().any(|event| {
        matches!(event, DomainEvent::WebSearchRecorded(record) if crate::connectors::hint::fires(record))
    });
    if hint && let Some(current) = current(actor, thread) {
        current.case_law_hint = crate::connectors::hint::TurnLooks::CaseLaw;
    }
    commit(actor, thread, events, Vec::new())
}

pub(super) fn tool_update(
    actor: &mut Actor,
    thread: ThreadId,
    context: Context,
    session: &AgentSessionId,
    acp_id: &str,
    mut patch: ToolCallPatch,
) -> Result<(), Error> {
    let found = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.sent.tool_call(session, acp_id))
        .map(|(id, tool)| (id, tool.fields));
    let Some((id, fields)) = found else {
        tracing::debug!(%thread, %acp_id, "an update for an unknown tool call was dropped");
        return Ok(());
    };
    if matches!(context, Context::Outside { replay: true }) {
        return Ok(());
    }
    // The turn's paths stay in memory, so they take the patch as the agent sent it. The
    // elicitation values withheld from this call (`withheld.rs`) then go before anything
    // logged reads it: the sources, and `build.rs`'s inline or blob decision.
    if let Some(current) = current(actor, thread) {
        if matches!(context, Context::Live(_)) {
            let fields = crate::sent::CallFields::of_patch(&patch);
            current.tool_paths.note(acp_id, &fields);
        }
        current.withheld.scrub_patch(acp_id, &mut patch);
    }
    let mut source = Vec::new();
    match context {
        Context::Outside { replay: true } => return Ok(()),
        Context::Live(turn) => {
            let now = actor.now().mono;
            if let Some(state) = state(actor, thread) {
                state.sources.codex_mcp_updated(acp_id, &patch);
            }
            let case_law = source_of(actor, thread, |sources, agent| {
                sources.case_law.updated(now, turn, acp_id, agent, &patch)
            });
            source = match case_law {
                Some(records) => records,
                None => source_of(actor, thread, |sources, agent| {
                    sources.updated(turn, acp_id, agent, &patch)
                })
                .into_iter()
                .collect(),
            };
        }
        Context::Outside { replay: false } => {}
    }
    let (built, _fields) = build::tool_call_updated(id, patch, fields);
    commit(actor, thread, [built.value.into()], built.blobs)?;
    sourced(actor, thread, source)
}

/// The tool call ACP knows as `acp_id` in `session`, if the host has it.
pub(in crate::actor::turn) fn known_tool(
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
