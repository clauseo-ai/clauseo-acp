//! The thread's sources: what the agent read on the web, recorded on the thread from the ACP tool
//! calls themselves (synthesis §3.3, seam 3: tool calls are looked up by name and shape) and shown
//! in the answer's sources list.
//!
//! - [`browser`]: the browser's own opens, `PageVisited { via: Browser }` (S27).
//! - [`claude`]: Claude Code's own `WebSearch` and `WebFetch` (S26): a search is
//!   `WebSearchRecorded`, a fetch is `PageVisited { via: WebFetch }`.
//! - [`codex`]: Codex's own `web_search` (S26): a search is `WebSearchRecorded` with no results,
//!   an opened or searched page is `PageVisited { via: CodexOpenPage }`.
//!
//! [`Sources`] is the thread's state for them. The host feeds it each tool call it logs in the live
//! turn (`actor/turn/updates.rs`) and logs the record it returns on the thread. Decision record
//! 0041 says which field each match uses.

pub(crate) mod browser;
mod calls;
pub(crate) mod claude;
pub(crate) mod codex;

#[cfg(test)]
mod tests;

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    AgentId, DomainEvent, PageVia, PageVisited, TextOrBlob, ToolCallId, ToolCallPatch, TurnId,
    WebResult, WebSearchRecorded,
};
use serde_json::Value;

/// The longest URL a search result may have, in characters, before the URL rule runs. A longer
/// one is dropped from the results (S26's own cap; the log's cap is larger).
pub(crate) const MAX_RESULT_URL_CHARS: usize = 2_000;

/// The thread's state for the sources: the browser's open calls and the agents' own web calls
/// that have not finished. Kept across turns and agent restarts.
#[derive(Debug, Default)]
pub(crate) struct Sources {
    browser: browser::Opens,
    calls: calls::Calls,
}

impl Sources {
    /// A tool call started in `turn` as `tool_call`, by `agent`: the record it adds, if it
    /// completed at once.
    pub(crate) fn started(
        &mut self,
        turn: TurnId,
        tool_call: ToolCallId,
        agent: &AgentId,
        call: &ToolCall,
    ) -> Option<DomainEvent> {
        let page = self
            .browser
            .started(turn, tool_call, call)
            .map(DomainEvent::from);
        let web = self.calls.started(turn, tool_call, agent, call);
        page.or(web)
    }

    /// An update of the call the agent knows as `acp_id`, logged in `turn`: the record it adds,
    /// when it completes the call.
    pub(crate) fn updated(
        &mut self,
        turn: TurnId,
        acp_id: &str,
        agent: &AgentId,
        patch: &ToolCallPatch,
    ) -> Option<DomainEvent> {
        let page = self
            .browser
            .updated(turn, acp_id, patch)
            .map(DomainEvent::from);
        let web = self.calls.updated(turn, acp_id, agent, patch);
        page.or(web)
    }
}

/// What a finished web tool call found, before it is an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// A search, with its query and the results the agent got.
    Search {
        query: String,
        results: Vec<WebResult>,
    },
    /// A page the agent read or opened.
    Page {
        url: String,
        title: Option<String>,
        via: PageVia,
    },
}

impl Found {
    /// The event it is, for the tool call `tool_call` of `turn` run by `agent`.
    pub(crate) fn into_event(
        self,
        turn: TurnId,
        tool_call: ToolCallId,
        agent: &AgentId,
    ) -> DomainEvent {
        match self {
            Self::Search { query, results } => {
                WebSearchRecorded::new(turn, tool_call, agent.clone(), &query, results).into()
            }
            Self::Page { url, title, via } => {
                PageVisited::new(turn, tool_call, &url, title, via).into()
            }
        }
    }
}

/// The query a search sent, from its input: `rawInput.query`, else `action.query`, else
/// `action.queries` joined with ", " (Codex's `action`). The first one that is not empty; none
/// when all are.
pub(crate) fn query_of(input: &Value) -> Option<String> {
    let text = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let action = input.get("action");
    let queries = action
        .and_then(|action| action.get("queries"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        });
    text(input.get("query"))
        .or_else(|| text(action.and_then(|action| action.get("query"))))
        .or_else(|| queries.filter(|joined| !joined.trim().is_empty()))
}

/// A tool result's text, from `rawOutput` as the agent sent it: a string; an array of blocks
/// joined with newlines; or an object with `text`, or with `content` or `result` holding one.
pub(crate) fn output_text(output: &Value) -> String {
    match output {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(output_text).collect::<Vec<_>>().join("\n"),
        Value::Object(_) => ["text", "content", "result"]
            .iter()
            .find_map(|key| output.get(*key))
            .map(output_text)
            .unwrap_or_default(),
        Value::Null | Value::Bool(_) | Value::Number(_) => String::new(),
    }
}

/// A title as the log keeps it: control characters and the bidi formatting characters a page
/// could spoof the sources list with are dropped, and the rest is trimmed.
pub(crate) fn clean_title(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Whether a URL holds only characters a record may keep: no whitespace, no control character
/// (C0, DEL, C1), and no bidi control (U+200E, U+200F, U+061C, U+202A to U+202E, U+2066 to U+2069).
/// A URL with one is refused, never cleaned: a bidi control could make a link read as another
/// address, and a space or control character is no part of a URL.
pub(crate) fn is_safe_url_text(url: &str) -> bool {
    !url.chars().any(|c| {
        c.is_whitespace()
            || c.is_control()
            || matches!(
                c,
                '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
            )
    })
}

/// The URL a record keeps, when `text` is one: an explicit `http://` or `https://` (in either
/// case) with a host after it. Nothing is added to it: a value with no scheme is no URL, so text
/// such as `Note (see below)` never becomes a link. The browser's address-bar rule in
/// `browser.rs`, which adds `https://`, is for the browser's own opens only.
pub(crate) fn record_url(text: &str) -> Option<String> {
    let rest = ["https://", "http://"].iter().find_map(|scheme| {
        let head = text.get(..scheme.len())?;
        head.eq_ignore_ascii_case(scheme)
            .then(|| &text[scheme.len()..])
    })?;
    (!rest.is_empty() && !rest.starts_with('/') && is_safe_url_text(text)).then(|| text.to_owned())
}

/// A tool call's `rawInput` or `rawOutput` as JSON: inline text only. A blob is too large to be
/// a web tool's and is passed over (the watcher reads events before the host spills them).
pub(crate) fn json_of(text: Option<&TextOrBlob>) -> Option<Value> {
    let TextOrBlob::Inline(text) = text? else {
        return None;
    };
    serde_json::from_str(text).ok()
}
