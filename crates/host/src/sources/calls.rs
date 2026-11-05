//! The agents' own web tool calls that have not finished (S26). A call is kept from its start until
//! it completes or fails, because the completion is where its input and output are whole: Claude's
//! first `tool_call` has an empty `rawInput`, and Codex's start item an empty query. Only the calls
//! that could be web calls are kept: kind `search` or `fetch`, or a name of `WebSearch` or
//! `WebFetch`. A call records only in the turn it started in: one that fails, is still open when its
//! turn ends, or completes in a later turn records nothing.

use std::collections::BTreeMap;

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    AgentId, CompactString, DomainEvent, TextOrBlob, ToolCallContent, ToolCallId, ToolCallPatch,
    ToolCallStatus, ToolKind, TurnId,
};
use serde_json::Value;

use super::{claude, codex, json_of};

/// The most unfinished web calls kept. Past it a new call is not tracked (and so records nothing),
/// and the calls already open keep their records; the calls of earlier turns are dropped as their
/// turn passes, so this bound is reached only by one turn's calls.
const MAX_CALLS: usize = 64;

/// The unfinished web calls of a thread, by the agent's tool call id.
#[derive(Debug, Default)]
pub(crate) struct Calls(BTreeMap<CompactString, Pending>);

/// One call as its latest start or update left it.
#[derive(Debug)]
struct Pending {
    /// The turn the call started in: the only turn it can record in.
    turn: TurnId,
    tool_call: ToolCallId,
    title: CompactString,
    tool_name: Option<CompactString>,
    kind: ToolKind,
    /// The last `rawInput`, as JSON.
    input: Option<Value>,
    /// The last `rawOutput`, as JSON.
    output: Option<Value>,
    /// The text of the last `content` that had any, one entry per text block.
    text: Vec<String>,
}

impl Calls {
    /// A tool call started in `turn` as `tool_call`: kept, or recorded at once when it started
    /// completed.
    pub(crate) fn started(
        &mut self,
        turn: TurnId,
        tool_call: ToolCallId,
        agent: &AgentId,
        call: &ToolCall,
    ) -> Option<DomainEvent> {
        // A call of `turn` ends the calls of the turns before it: those can no longer record.
        self.0.retain(|_, pending| pending.turn == turn);
        if !is_candidate(call.kind, call.tool_name.as_deref()) {
            return None;
        }
        let pending = Pending::of(turn, tool_call, call);
        match call.status {
            ToolCallStatus::Completed => finished(turn, agent, &pending),
            ToolCallStatus::Failed | ToolCallStatus::Cancelled => None,
            ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => {
                if self.0.len() >= MAX_CALLS {
                    // The calls already open keep their records; this one is not tracked.
                    tracing::warn!(acp_id = %call.acp_id, "too many unfinished web calls; not tracked");
                    return None;
                }
                self.0.insert(call.acp_id.clone(), pending);
                None
            }
        }
    }

    /// An update of the call the agent knows as `acp_id`, logged in `turn`: its fields replace
    /// the call's; `completed` records what the call found, `failed` or `cancelled` forgets it.
    pub(crate) fn updated(
        &mut self,
        turn: TurnId,
        acp_id: &str,
        agent: &AgentId,
        patch: &ToolCallPatch,
    ) -> Option<DomainEvent> {
        let pending = self.0.get_mut(acp_id)?;
        if pending.turn != turn {
            // The call began in an earlier turn: it records nothing, and is forgotten.
            self.0.remove(acp_id);
            return None;
        }
        pending.absorb(patch);
        match patch.status? {
            ToolCallStatus::Completed => {
                let pending = self.0.remove(acp_id)?;
                finished(turn, agent, &pending)
            }
            ToolCallStatus::Failed | ToolCallStatus::Cancelled => {
                self.0.remove(acp_id);
                None
            }
            ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => None,
        }
    }
}

/// Whether a call could be one of the agents' web calls, by its kind and name.
fn is_candidate(kind: ToolKind, name: Option<&str>) -> bool {
    matches!(kind, ToolKind::Search | ToolKind::Fetch)
        || matches!(name, Some(claude::SEARCH | claude::FETCH))
}

impl Pending {
    fn of(turn: TurnId, tool_call: ToolCallId, call: &ToolCall) -> Self {
        Self {
            turn,
            tool_call,
            title: call.title.clone(),
            tool_name: call.tool_name.clone(),
            kind: call.kind,
            input: json_of(call.raw_input.as_ref()),
            output: json_of(call.raw_output.as_ref()),
            text: text_of(&call.content),
        }
    }

    /// The fields an update carries replace the call's.
    fn absorb(&mut self, patch: &ToolCallPatch) {
        if let Some(title) = &patch.title {
            self.title.clone_from(title);
        }
        if let Some(name) = &patch.tool_name {
            self.tool_name = Some(name.clone());
        }
        if let Some(kind) = patch.kind {
            self.kind = kind;
        }
        if patch.raw_input.is_some() {
            self.input = json_of(patch.raw_input.as_ref());
        }
        if patch.raw_output.is_some() {
            self.output = json_of(patch.raw_output.as_ref());
        }
        if let Some(content) = &patch.content {
            let text = text_of(content);
            if !text.is_empty() {
                self.text = text;
            }
        }
    }
}

/// The text blocks of `content`, in order.
fn text_of(content: &[ToolCallContent]) -> Vec<String> {
    content
        .iter()
        .filter_map(|item| match item {
            ToolCallContent::Text {
                text: TextOrBlob::Inline(text),
            } => Some(text.clone()),
            ToolCallContent::Text { .. }
            | ToolCallContent::Content { .. }
            | ToolCallContent::Diff { .. }
            | ToolCallContent::Unknown { .. } => None,
        })
        .collect()
}

/// The event a finished call adds, by which tool it is: Claude's by its name (or, with none, its
/// title), else Codex's search by its shape. Codex's search has no name, so a named call with
/// kind `search` is no Codex search (an MCP server's own search tool, say).
fn finished(turn: TurnId, agent: &AgentId, pending: &Pending) -> Option<DomainEvent> {
    let input = pending.input.as_ref();
    let found = match claude::tool(
        pending.kind,
        pending.tool_name.as_deref(),
        &pending.title,
        input,
    ) {
        Some(claude::Tool::Search) => claude::search(input, pending.output.as_ref(), &pending.text),
        Some(claude::Tool::Fetch) => claude::fetch(input),
        None if pending.tool_name.is_none() && codex::is_search(pending.kind, input) => {
            codex::found(input?)
        }
        None => None,
    };
    found.map(|found| found.into_event(turn, pending.tool_call, agent))
}
