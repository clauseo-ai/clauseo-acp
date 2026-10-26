//! Agent traffic into `protocol`'s types: one function per thing, no I/O and no state.

mod content;
mod limit;
mod session;
mod tool;

#[cfg(test)]
mod tests;

use std::path::Path;

use agent_client_protocol::schema::MaybeUndefined;
use agent_client_protocol::schema::v1 as acp;
use clauseo_protocol::{AgentSessionId, CompactString, ContentBlock, Cost, Count, Finite};
use serde_json::Value;

use crate::event::{AcpEvent, AgentUpdate, MessageRole, Origin, ProtocolWarning};

pub(crate) use content::{prompt_block, split_text};
pub(crate) use limit::session_failure;
pub(crate) use session::{agent_info, config_options, modes, stop_reason};

/// The `sessionUpdate` names this crate maps: every stable variant of schema 1.11.0. Anything
/// else (`plan_update` and `plan_removed` until codex-acp's plans are mapped, the AIR
/// `subagent_*` and `async_task_*` updates, a newer variant) is skipped with
/// [`ProtocolWarning::UnknownUpdate`] before the update is decoded.
pub(crate) const KNOWN_UPDATES: [&str; 14] = [
    "user_message_chunk",
    "agent_message_chunk",
    "agent_thought_chunk",
    "tool_call",
    "tool_call_update",
    "plan",
    "available_commands_update",
    "current_mode_update",
    "config_option_update",
    "session_info_update",
    "usage_update",
    "notice",
    "compaction_update",
    "compaction_summary_chunk",
];

/// The ACP content block types schema 1.11.0 decodes.
const KNOWN_BLOCKS: [&str; 5] = ["text", "image", "audio", "resource_link", "resource"];

/// Where a content block of a type the SDK cannot decode sat in an update.
#[derive(Debug, PartialEq)]
pub(crate) enum Quarantined {
    /// The `content` of a message or summary chunk.
    Chunk(ContentBlock),
    /// Item `index` of a tool call's `content` list.
    ToolContent(usize, ContentBlock),
}

/// Replaces each content block whose `type` the SDK does not know with an empty text block, so
/// the update still decodes, and returns each one as `ContentBlock::Unsupported` holding the
/// block's JSON. Looks only where a block can sit; an update with none allocates nothing.
pub(crate) fn quarantine_unknown_blocks(update: &mut Value) -> Vec<Quarantined> {
    let mut found = Vec::new();
    let Some(update) = update.as_object_mut() else {
        return found;
    };
    match update.get("sessionUpdate").and_then(Value::as_str) {
        Some(
            "user_message_chunk"
            | "agent_message_chunk"
            | "agent_thought_chunk"
            | "compaction_summary_chunk",
        ) => {
            if let Some(block) = update.get_mut("content")
                && let Some(unknown) = take_unknown(block)
            {
                found.push(Quarantined::Chunk(unknown));
            }
        }
        Some("tool_call" | "tool_call_update") => {
            let items = update.get_mut("content").and_then(Value::as_array_mut);
            for (index, item) in items.into_iter().flatten().enumerate() {
                if item.get("type").and_then(Value::as_str) == Some("content")
                    && let Some(block) = item.get_mut("content")
                    && let Some(unknown) = take_unknown(block)
                {
                    found.push(Quarantined::ToolContent(index, unknown));
                }
            }
        }
        Some("compaction_update") => {
            // The summary list is not mapped (the summary chunks are); an unknown block in it
            // must not fail the status.
            let items = update.get_mut("summary").and_then(Value::as_array_mut);
            for block in items.into_iter().flatten() {
                drop(take_unknown(block));
            }
        }
        _ => {}
    }
    found
}

fn take_unknown(block: &mut Value) -> Option<ContentBlock> {
    let kind = block.get("type").and_then(Value::as_str)?;
    if KNOWN_BLOCKS.contains(&kind) {
        return None;
    }
    let kind = kind.to_owned();
    let whole = std::mem::replace(block, serde_json::json!({"type": "text", "text": ""}));
    Some(content::unsupported(kind, &whole))
}

/// Maps one decoded `session/update` into events, pushed onto `out`. `cwd` is the session's
/// folder, for tool locations; `quarantined` the blocks [`quarantine_unknown_blocks`] took out.
/// The last event takes `session`; only an update that makes several events clones it.
pub(crate) fn update(
    session: AgentSessionId,
    origin: Origin,
    update: acp::SessionUpdate,
    cwd: Option<&Path>,
    quarantined: Vec<Quarantined>,
    out: &mut Vec<AcpEvent>,
) {
    let mut quarantined = quarantined.into_iter();
    let mapped = match update {
        acp::SessionUpdate::UserMessageChunk(chunk) => {
            let role = MessageRole::User;
            return chunks(session, origin, role, chunk, quarantined.next(), out);
        }
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            let role = MessageRole::Answer;
            return chunks(session, origin, role, chunk, quarantined.next(), out);
        }
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            let role = MessageRole::Thought;
            return chunks(session, origin, role, chunk, quarantined.next(), out);
        }
        acp::SessionUpdate::ToolCall(call) => {
            AgentUpdate::ToolCall(tool::tool_call(call, cwd, tool_blocks(quarantined)))
        }
        acp::SessionUpdate::ToolCallUpdate(update) => AgentUpdate::ToolCallUpdate {
            acp_id: CompactString::from(&*update.tool_call_id.0),
            patch: tool::patch(update.fields, cwd, tool_blocks(quarantined)),
        },
        acp::SessionUpdate::Plan(plan) => AgentUpdate::Plan(session::plan(plan.entries)),
        acp::SessionUpdate::AvailableCommandsUpdate(update) => {
            AgentUpdate::Commands(session::commands(update.available_commands))
        }
        acp::SessionUpdate::CurrentModeUpdate(update) => AgentUpdate::CurrentMode {
            mode_id: CompactString::from(&*update.current_mode_id.0),
        },
        acp::SessionUpdate::ConfigOptionUpdate(update) => {
            match session::config_options(update.config_options) {
                Ok(options) => AgentUpdate::ConfigOptions(options),
                Err(error) => {
                    return out.push(invalid(Some(session), Some("config_option_update"), error));
                }
            }
        }
        acp::SessionUpdate::SessionInfoUpdate(update) => {
            let failure = update.meta.as_ref().and_then(limit::session_failure);
            let title = match update.title {
                MaybeUndefined::Value(title) => Some(Some(title)),
                MaybeUndefined::Null => Some(None),
                MaybeUndefined::Undefined => None,
            };
            match (failure, title) {
                (Some(failure), Some(title)) => {
                    out.push(limit_event(session.clone(), origin, failure));
                    AgentUpdate::SessionTitle { title }
                }
                (Some(failure), None) => return out.push(limit_event(session, origin, failure)),
                (None, Some(title)) => AgentUpdate::SessionTitle { title },
                (None, None) => return,
            }
        }
        acp::SessionUpdate::UsageUpdate(update) => return usage(session, origin, update, out),
        acp::SessionUpdate::Notice(notice) => session::notice(notice),
        acp::SessionUpdate::CompactionUpdate(update) => match compaction(&update) {
            Ok(update) => update,
            Err(error) => {
                return out.push(invalid(Some(session), Some("compaction_update"), error));
            }
        },
        acp::SessionUpdate::CompactionSummaryChunk(chunk) => {
            let acp_id = CompactString::from(&*chunk.compaction_id.0);
            let block = match quarantined.next() {
                Some(Quarantined::Chunk(block)) => block,
                Some(Quarantined::ToolContent(..)) | None => content::block(chunk.content),
            };
            return push_parts(
                block,
                (session, acp_id),
                out,
                |(session, acp_id), content| {
                    let update = AgentUpdate::CompactionSummaryChunk { acp_id, content };
                    update_event(session, origin, update)
                },
            );
        }
        // Filtered out before decoding (`KNOWN_UPDATES`); kept for a change of that list.
        acp::SessionUpdate::PlanUpdate(_) => {
            return out.push(unknown(Some(session), "plan_update"));
        }
        acp::SessionUpdate::PlanRemoved(_) => {
            return out.push(unknown(Some(session), "plan_removed"));
        }
        other => {
            let whole = serde_json::to_value(&other).unwrap_or_default();
            let name = whole.get("sessionUpdate").and_then(Value::as_str);
            let event = if let Some(name) = name {
                unknown(Some(session), name)
            } else {
                let error = "an update newer than schema 1.11.0".to_owned();
                invalid(Some(session), None, error)
            };
            return out.push(event);
        }
    };
    out.push(update_event(session, origin, mapped));
}

fn update_event(session: AgentSessionId, origin: Origin, update: AgentUpdate) -> AcpEvent {
    AcpEvent::Update {
        session,
        origin,
        update,
    }
}

fn chunks(
    session: AgentSessionId,
    origin: Origin,
    role: MessageRole,
    chunk: acp::ContentChunk,
    quarantined: Option<Quarantined>,
    out: &mut Vec<AcpEvent>,
) {
    let acp_message_id = chunk.message_id.map(|id| CompactString::from(&*id.0));
    let block = match quarantined {
        Some(Quarantined::Chunk(block)) => block,
        Some(Quarantined::ToolContent(..)) | None => content::block(chunk.content),
    };
    let shared = (session, acp_message_id);
    push_parts(block, shared, out, |(session, acp_message_id), content| {
        let update = AgentUpdate::MessageChunk {
            role,
            acp_message_id,
            content,
        };
        update_event(session, origin, update)
    });
}

/// The usage, then Claude's rate limit when its `_meta` carries one.
fn usage(
    session: AgentSessionId,
    origin: Origin,
    update: acp::UsageUpdate,
    out: &mut Vec<AcpEvent>,
) {
    let cost = update.cost.and_then(|cost| {
        Finite::new(cost.amount).map(|amount| Cost {
            amount,
            currency: cost.currency,
        })
    });
    let usage = AgentUpdate::Usage {
        used: Count::clamped(update.used),
        size: Count::clamped(update.size),
        cost,
    };
    match update.meta.as_ref().and_then(limit::rate_limit) {
        Some(signal) => {
            out.push(update_event(session.clone(), origin, usage));
            out.push(limit_event(session, origin, signal));
        }
        None => out.push(update_event(session, origin, usage)),
    }
}

fn compaction(update: &acp::CompactionUpdate) -> Result<AgentUpdate, String> {
    let acp_id = CompactString::from(&*update.compaction_id.0);
    let completed = match &update.status {
        acp::CompactionStatus::InProgress => return Ok(AgentUpdate::CompactionStarted { acp_id }),
        acp::CompactionStatus::Completed => true,
        acp::CompactionStatus::Failed | acp::CompactionStatus::Cancelled => false,
        acp::CompactionStatus::Other(_) | _ => {
            return Err(format!("unknown compaction status {:?}", update.status));
        }
    };
    Ok(AgentUpdate::CompactionEnded { acp_id, completed })
}

/// Pushes the event `make` builds for each part of `block`: a text block over 1 MiB in several
/// parts, any other block as itself. The one part of a block that is not split takes `shared`;
/// the parts of a split block each get a clone.
fn push_parts<T: Clone>(
    block: ContentBlock,
    shared: T,
    out: &mut Vec<AcpEvent>,
    make: impl Fn(T, ContentBlock) -> AcpEvent,
) {
    match block {
        ContentBlock::Text { text } if text.len() > content::MAX_TEXT_CHUNK_BYTES => {
            for text in split_text(&text) {
                out.push(make(shared.clone(), ContentBlock::Text { text }));
            }
        }
        block @ (ContentBlock::Text { .. }
        | ContentBlock::Image { .. }
        | ContentBlock::Audio { .. }
        | ContentBlock::ResourceLink { .. }
        | ContentBlock::Resource { .. }
        | ContentBlock::Unsupported { .. }) => out.push(make(shared, block)),
    }
}

/// The tool-content blocks among `quarantined`, by item index.
fn tool_blocks(quarantined: impl Iterator<Item = Quarantined>) -> Vec<(usize, ContentBlock)> {
    quarantined
        .filter_map(|item| match item {
            Quarantined::ToolContent(index, block) => Some((index, block)),
            Quarantined::Chunk(_) => None,
        })
        .collect()
}

fn limit_event(
    session: AgentSessionId,
    origin: Origin,
    signal: Result<crate::event::LimitSignal, ProtocolWarning>,
) -> AcpEvent {
    match signal {
        Ok(signal) => AcpEvent::Limit {
            session,
            origin,
            signal,
        },
        Err(warning) => warning_event(Some(session), warning),
    }
}

/// A warning, about `session` when it is known.
pub(crate) fn warning_event(session: Option<AgentSessionId>, warning: ProtocolWarning) -> AcpEvent {
    AcpEvent::Warning { session, warning }
}

pub(crate) fn unknown(session: Option<AgentSessionId>, name: &str) -> AcpEvent {
    let name = name.to_owned();
    warning_event(session, ProtocolWarning::UnknownUpdate { name })
}

pub(crate) fn invalid(
    session: Option<AgentSessionId>,
    name: Option<&str>,
    error: String,
) -> AcpEvent {
    let name = name.map(str::to_owned);
    warning_event(session, ProtocolWarning::InvalidUpdate { name, error })
}
