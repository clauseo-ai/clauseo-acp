//! Tool calls and their updates: a live turn's are logged with what they add to the thread's
//! sources; outside a turn a new one is dropped and an update of a known one logged as is.

use clauseo_protocol::{
    AgentId, AgentSessionId, CompactString, DomainEvent, StreamId, ThreadId, ToolCallPatch,
};

use super::super::super::Actor;
use super::super::helpers::{self, claude::Before};
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
    // The session's launch read whether Claude Code's policy settings may keep its shell's
    // folder across calls: then no shell call's starting folder is known.
    let start = agent(actor, thread).map(|agent| agent.shell_start.clone());
    if let Some(current) = current(actor, thread) {
        if let Some(start) = start {
            current.tool_paths.set_start(start);
        }
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
    let folder = fallback_folder(actor, thread);
    // The helper a stamped step belongs to (`helpers.rs`), read with the fallbacks so a
    // conversation without helpers pays no lookup of its own.
    let (saved, helper) = state(actor, thread)
        .map(|state| {
            let saved = state.sources.fallbacks.started(&call, folder.as_deref());
            let helper = state
                .helpers
                .as_deref_mut()
                .and_then(|helpers| helpers.step_of(&call.acp_id));
            (Some(saved), helper)
        })
        .unwrap_or_default();
    // A launch keeps what its helper starts with before the build takes the call.
    let launch = helpers::claude::is_launch(call.tool_name.as_deref()).then(|| {
        let launch = helpers::claude::Launch::new(id, turn, call.title.clone(), helper);
        (call.acp_id.clone(), launch, call.raw_input.clone())
    });
    let (mut built, _fields) = build::tool_call_started(turn, id, call);
    built.value.helper = helper;
    commit(actor, thread, [built.value.into()], built.blobs)?;
    if let Some((acp_id, launch, raw_input)) = launch {
        helpers::claude::launched(actor, thread, &acp_id, launch, raw_input.as_ref())?;
    }
    sourced(
        actor,
        thread,
        source
            .into_iter()
            .chain(case_law.into_iter().flatten())
            .chain(saved.into_iter().flatten().map(DomainEvent::from)),
    )
}

/// Clauseo's own downloads folder for the matter of the thread's running session
/// (`agent::loads::browser::fallback_downloads`), as its guard got it; none when the thread
/// has no session folders, and then no result is read for saved files.
fn fallback_folder(actor: &mut Actor, thread: ThreadId) -> Option<std::path::PathBuf> {
    let run = state(actor, thread)?.run.as_ref()?;
    Some(crate::agent::loads::browser::fallback_downloads(&run.notes))
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
    let replay = matches!(context, Context::Outside { replay: true });
    // One look at the thread's state: whether the conversation has helpers, and for a known
    // call the turn's paths, which take the patch as the agent sent it. The elicitation values
    // withheld from this call (`withheld.rs`) then go before anything logged reads it: the
    // sources, the helpers, and `build.rs`'s inline or blob decision.
    let mut has_helpers = false;
    if !replay && let Some(state) = state(actor, thread) {
        has_helpers = state.helpers.is_some();
        if found.is_some()
            && let Some(current) = state.current.as_deref_mut()
        {
            if matches!(context, Context::Live(_)) {
                let fields = crate::sent::CallFields::of_patch(&patch);
                current.tool_paths.note(acp_id, &fields);
            }
            current.withheld.scrub_patch(acp_id, &mut patch);
        }
    }
    // A helper's handback is no step: its update is read, not logged (`helpers/claude.rs`).
    let before = if has_helpers {
        helpers::claude::before_update(actor, thread, acp_id, &patch)?
    } else {
        Before::Plain
    };
    if matches!(before, Before::Swallow) {
        return Ok(());
    }
    let Some((id, fields)) = found else {
        tracing::debug!(%thread, %acp_id, "an update for an unknown tool call was dropped");
        return Ok(());
    };
    if replay {
        return Ok(());
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
            let folder = fallback_folder(actor, thread);
            if let Some(state) = state(actor, thread) {
                let saved = state
                    .sources
                    .fallbacks
                    .updated(acp_id, &patch, folder.as_deref());
                source.extend(saved.into_iter().map(DomainEvent::from));
            }
        }
        Context::Outside { replay: false } => {}
    }
    let (built, _fields) = build::tool_call_updated(id, patch, fields);
    commit(actor, thread, [built.value.into()], built.blobs)?;
    if let Before::Launch(launch) = before {
        helpers::claude::after_launch_update(actor, thread, &CompactString::from(acp_id), launch)?;
    }
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
