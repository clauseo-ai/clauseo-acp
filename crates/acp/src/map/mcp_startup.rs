//! codex-acp's report of an MCP server that did not start: a `tool_call` it makes up, not a step
//! of the agent's, so it becomes a warning instead.
//!
//! codex-acp 2.10.0 (`McpStartupReporter`, `dist/index.js:42387-42420`) sends one `tool_call` per
//! server that failed, timed out, was cancelled or was skipped at start-up: `toolCallId`
//! `mcp_startup.<name, percent-encoded>.<uuid>`, `title` `mcp__<name>__startup`, `status`
//! `failed`, `kind` `other`, and one text block, the adapter's words and Codex's error. Each id is
//! new ("so that a later report for the same server cannot replace this one"), and no update
//! for it was seen in the recorded runs; one with that id and `failed` would go the same way.
//! The report comes when the server's start-up ends, which for a server that never answers is
//! Codex's 30 s start-up timeout: after the `session/new` answer, possibly inside a turn.

use agent_client_protocol::schema::v1 as acp;

use crate::event::{McpStartupReason, ProtocolWarning};

/// The start of the id codex-acp gives every start-up report.
const ID_PREFIX: &str = "mcp_startup.";
/// The title of a start-up report is `mcp__<name>__startup`.
const TITLE_PREFIX: &str = "mcp__";
const TITLE_SUFFIX: &str = "__startup";

/// The warning for `call` when it is a start-up report: its id, title and status all of that
/// shape. `None` for any other call, which stays a step.
pub(crate) fn failed_call(call: &acp::ToolCall) -> Option<ProtocolWarning> {
    if !call.tool_call_id.0.starts_with(ID_PREFIX) || call.status != acp::ToolCallStatus::Failed {
        return None;
    }
    Some(ProtocolWarning::McpServerNotStarted {
        server: server_of_title(&call.title)?,
        reason: reason(text(&call.content)),
    })
}

/// The warning for `update` when it fails a start-up report: an id of that shape, `failed`,
/// and a title of that shape if it has one. The server's name then comes from the title, else
/// from the id, as codex-acp percent-encoded it. `None` for any other update.
pub(crate) fn failed_update(update: &acp::ToolCallUpdate) -> Option<ProtocolWarning> {
    let fields = &update.fields;
    let rest = update.tool_call_id.0.strip_prefix(ID_PREFIX)?;
    if fields.status != Some(acp::ToolCallStatus::Failed) {
        return None;
    }
    let server = match &fields.title {
        Some(title) => server_of_title(title)?,
        None => rest
            .rsplit_once('.')
            .map(|(name, _)| name)
            .unwrap_or(rest)
            .to_owned(),
    };
    let content = fields.content.as_deref().unwrap_or_default();
    Some(ProtocolWarning::McpServerNotStarted {
        server,
        reason: reason(text(content)),
    })
}

/// `<name>` of `mcp__<name>__startup`; `None` for any other title or an empty name.
fn server_of_title(title: &str) -> Option<String> {
    let name = title
        .strip_prefix(TITLE_PREFIX)?
        .strip_suffix(TITLE_SUFFIX)?;
    (!name.is_empty()).then(|| name.to_owned())
}

/// The first text block of `content`.
fn text(content: &[acp::ToolCallContent]) -> Option<&str> {
    content.iter().find_map(|item| match item {
        acp::ToolCallContent::Content(content) => match &content.content {
            acp::ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        },
        _ => None,
    })
}

/// Why, read from the adapter's own words in the report (`McpStartupReporter`): "startup was
/// cancelled", "was not started, because the Codex config already defines an MCP server with
/// this name", and for a failure Codex's error, which says "timed out after" for its start-up
/// timeout. Nothing else of the text is kept: Codex's error can hold a path or a command.
fn reason(text: Option<&str>) -> McpStartupReason {
    let text = text.unwrap_or_default();
    if text.contains("startup was cancelled") {
        McpStartupReason::Cancelled
    } else if text.contains("was not started, because") {
        McpStartupReason::NameTaken
    } else if text.contains("timed out after") {
        McpStartupReason::TimedOut
    } else {
        McpStartupReason::Failed
    }
}
