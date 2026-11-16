//! The case-law connector's sources (R24, PR6): `WebSearchRecorded { source: CaseLaw }` for a
//! `search_law` call, `PageVisited { via: CaseLaw }` for each document a `read_document` call
//! returned.
//!
//! The join: the relay sees the JSON-RPC call and its result, the agent's ACP tool call carries
//! the `tool_call_id`, and S26 records on the completed `tool_call_update`. So the relay hands
//! the host what each call found ([`CaseLawFound`]), keyed by SHA-256 of the tool's name and the
//! canonical JSON of its arguments ([`join_key`]: keys sorted, compact), and the thread keeps it
//! for [`ENTRY_LIFE`], or until a turn of the thread ends (`actor/turn/end.rs`), whichever
//! comes first. When the agent's call completes in the live turn, its own arguments give the
//! key, and the entry it finds is taken (used once).
//!
//! The tool is looked up by name, as each agent reports it (as `browser.rs` does):
//! - Claude: `mcp__case-law__<tool>` in the call's `name` or `title`; `rawInput` is the
//!   arguments;
//! - Codex: the title `mcp.case-law.<tool>` (codex-acp 2.2.2); `rawInput` is
//!   `{ server, tool, arguments }`.
//!
//! No entry (the relay's result came after the update, or more than five minutes before it):
//! a search still records its query, with no results; a read records nothing. A call whose
//! result is an error (`isError`, Codex's `error`) records nothing. `citator`,
//! `check_citations`, `account` and `recharge` record nothing. `Origin::Late` and
//! `Origin::Replay` never reach here (`actor/turn/updates.rs`), so a replay adds nothing.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    AgentId, CompactString, DomainEvent, MAX_SEARCH_QUERY_CHARS, PageVia, PageVisited,
    SearchSource, ToolCallId, ToolCallPatch, ToolCallStatus, TurnId, WebResult, WebSearchRecorded,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::{clean_title, json_of, record_url};
use crate::connectors::{CaseLawFound, CaseLawTool, FoundLink};

/// The server's prefix in Claude's tool names.
pub(crate) const CLAUDE_PREFIX: &str = "mcp__case-law__";
/// The server's prefix in codex-acp 2.2.2's MCP titles.
pub(crate) const CODEX_PREFIX: &str = "mcp.case-law.";
/// How long the relay's result waits for the agent's tool call to complete.
pub(crate) const ENTRY_LIFE: Duration = Duration::from_mins(5);
/// The most results a thread holds; past it the oldest goes.
const MAX_ENTRIES: usize = 64;
/// The most unfinished case-law calls a thread follows; past it they are let go.
const MAX_CALLS: usize = 64;

/// The join key: SHA-256 of the tool's name, a zero byte, and the canonical JSON of its
/// arguments.
pub(crate) fn join_key(tool: CaseLawTool, arguments: &Value) -> [u8; 32] {
    let mut text = String::new();
    canonical(arguments, &mut text);
    let mut hash = Sha256::new();
    hash.update(tool.name().as_bytes());
    hash.update([0]);
    hash.update(text.as_bytes());
    hash.finalize().into()
}

/// `value` as compact JSON with every object's keys sorted, whatever order the map keeps
/// (the workspace's `serde_json` keeps insertion order).
pub(crate) fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                if let Some(item) = map.get(key) {
                    canonical(item, out);
                }
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            out.push_str(&value.to_string());
        }
    }
}

/// A thread's case-law state: the relay's results waiting for their tool call, and the
/// case-law calls that have not finished.
#[derive(Debug, Default)]
pub(crate) struct CaseLaw {
    entries: VecDeque<Entry>,
    calls: BTreeMap<CompactString, Pending>,
}

#[derive(Debug)]
struct Entry {
    at: Duration,
    found: CaseLawFound,
}

/// One case-law call as its latest start or update left it.
#[derive(Debug)]
struct Pending {
    turn: TurnId,
    tool_call: ToolCallId,
    tool: CaseLawTool,
    /// Codex's form: the arguments sit under `arguments`.
    codex: bool,
    input: Option<Value>,
    output: Option<Value>,
}

impl CaseLaw {
    /// The relay found `found` at `now` (the host's monotonic clock).
    pub(crate) fn found(&mut self, now: Duration, found: CaseLawFound) {
        self.expire(now);
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry { at: now, found });
    }

    /// A tool call started: `None` when it is no case-law call this module records (the other
    /// sources then look at it), else the records it adds (some only if it started completed).
    pub(crate) fn started(
        &mut self,
        now: Duration,
        turn: TurnId,
        tool_call: ToolCallId,
        agent: &AgentId,
        call: &ToolCall,
    ) -> Option<Vec<DomainEvent>> {
        // A call of `turn` ends the calls of the turns before it: those can no longer record.
        self.calls.retain(|_, pending| pending.turn == turn);
        let (tool, codex) = tool_of(&call.title, call.tool_name.as_deref())?;
        let pending = Pending {
            turn,
            tool_call,
            tool,
            codex,
            input: json_of(call.raw_input.as_ref()),
            output: json_of(call.raw_output.as_ref()),
        };
        Some(match call.status {
            ToolCallStatus::Completed => self.finished(now, agent, &pending),
            ToolCallStatus::Failed | ToolCallStatus::Cancelled => Vec::new(),
            ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => {
                if self.calls.len() >= MAX_CALLS {
                    // Calls an agent never finished are let go.
                    self.calls.clear();
                }
                self.calls.insert(call.acp_id.clone(), pending);
                Vec::new()
            }
        })
    }

    /// An update of the call the agent knows as `acp_id`, logged in `turn`: `None` when it is
    /// no case-law call this module follows, else the records it adds.
    pub(crate) fn updated(
        &mut self,
        now: Duration,
        turn: TurnId,
        acp_id: &str,
        agent: &AgentId,
        patch: &ToolCallPatch,
    ) -> Option<Vec<DomainEvent>> {
        let pending = self.calls.get_mut(acp_id)?;
        if pending.turn != turn {
            self.calls.remove(acp_id);
            return Some(Vec::new());
        }
        if patch.raw_input.is_some() {
            pending.input = json_of(patch.raw_input.as_ref());
        }
        if patch.raw_output.is_some() {
            pending.output = json_of(patch.raw_output.as_ref());
        }
        Some(match patch.status {
            Some(ToolCallStatus::Completed) => match self.calls.remove(acp_id) {
                Some(pending) => self.finished(now, agent, &pending),
                None => Vec::new(),
            },
            Some(ToolCallStatus::Failed | ToolCallStatus::Cancelled) => {
                self.calls.remove(acp_id);
                Vec::new()
            }
            Some(
                ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown,
            )
            | None => Vec::new(),
        })
    }

    /// A turn of the thread ended: the relay's results no call of it took are let go, since a
    /// later turn's call is another call, even with the same arguments.
    pub(crate) fn turn_ended(&mut self) {
        self.entries.clear();
    }

    /// Drops the results older than [`ENTRY_LIFE`].
    fn expire(&mut self, now: Duration) {
        self.entries
            .retain(|entry| now.saturating_sub(entry.at) <= ENTRY_LIFE);
    }

    /// Takes the entry for `tool` with `key`, if one is young enough.
    fn take(&mut self, now: Duration, tool: CaseLawTool, key: &[u8; 32]) -> Option<CaseLawFound> {
        self.expire(now);
        let index = self
            .entries
            .iter()
            .position(|entry| entry.found.tool == tool && entry.found.key == *key)?;
        self.entries.remove(index).map(|entry| entry.found)
    }

    /// The records a completed call adds.
    fn finished(&mut self, now: Duration, agent: &AgentId, call: &Pending) -> Vec<DomainEvent> {
        if call.output.as_ref().is_some_and(is_error) {
            tracing::debug!(tool_call = %call.tool_call, "a case-law call that failed is not recorded");
            return Vec::new();
        }
        let arguments = arguments_of(call);
        let entry = self.take(now, call.tool, &join_key(call.tool, &arguments));
        let links = entry.map(|found| found.links).unwrap_or_default();
        match call.tool {
            CaseLawTool::SearchLaw => {
                let Some(query) = query_of(&arguments) else {
                    return Vec::new();
                };
                let results = links.iter().filter_map(result_of).collect();
                let record = WebSearchRecorded::new(
                    call.turn,
                    call.tool_call,
                    agent.clone(),
                    &query,
                    results,
                )
                .with_source(SearchSource::CaseLaw, Some(call.tool.name()));
                vec![record.into()]
            }
            CaseLawTool::ReadDocument => links
                .iter()
                .filter_map(result_of)
                .map(|page| {
                    let title = Some(page.title).filter(|title| *title != page.url);
                    PageVisited::new(
                        call.turn,
                        call.tool_call,
                        &page.url,
                        title,
                        PageVia::CaseLaw,
                    )
                    .into()
                })
                .collect(),
        }
    }
}

/// The case-law tool a call is, by its name or title, and whether it is Codex's form.
fn tool_of(title: &str, tool_name: Option<&str>) -> Option<(CaseLawTool, bool)> {
    [tool_name, Some(title)]
        .into_iter()
        .flatten()
        .find_map(|name| {
            if let Some(tool) = name.strip_prefix(CLAUDE_PREFIX) {
                CaseLawTool::of(tool).map(|tool| (tool, false))
            } else {
                let tool = name.strip_prefix(CODEX_PREFIX)?;
                CaseLawTool::of(tool).map(|tool| (tool, true))
            }
        })
}

/// The arguments the agent sent: `rawInput` (Claude), or its `arguments` (Codex); `{}` when
/// there are none.
fn arguments_of(call: &Pending) -> Value {
    let input = if call.codex {
        call.input.as_ref().and_then(|input| input.get("arguments"))
    } else {
        call.input.as_ref()
    };
    input
        .filter(|input| !input.is_null())
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
}

/// A search's query: `query`, or the searches of `queries` or `searches` (strings, or objects
/// with a `query`) joined with "; ", each cleaned as a title is; capped at
/// [`MAX_SEARCH_QUERY_CHARS`]. `search_law` takes up to five searches per call; the live
/// argument names are not checked yet, so each form is read.
pub(crate) fn query_of(arguments: &Value) -> Option<String> {
    let text = |value: &Value| {
        value
            .as_str()
            .or_else(|| value.get("query").and_then(Value::as_str))
            .map(clean_title)
            .filter(|text| !text.is_empty())
    };
    let query = arguments.get("query").and_then(text).or_else(|| {
        let searches: Vec<String> = ["queries", "searches"]
            .iter()
            .filter_map(|key| arguments.get(*key)?.as_array())
            .flatten()
            .filter_map(text)
            .collect();
        (!searches.is_empty()).then(|| searches.join("; "))
    })?;
    Some(query.chars().take(MAX_SEARCH_QUERY_CHARS).collect())
}

/// A found link as a record keeps it: a web URL ([`record_url`]) and a cleaned title, or the
/// URL as its title when the title is empty.
fn result_of(link: &FoundLink) -> Option<WebResult> {
    let url = record_url(&link.url)?;
    let title = clean_title(&link.title);
    let title = if title.is_empty() { url.clone() } else { title };
    Some(WebResult { title, url })
}

/// Whether the MCP result in `rawOutput` says the call failed: a non-null `error` (Codex), or
/// `isError: true` at its top level or under `result`.
fn is_error(output: &Value) -> bool {
    let flagged = |value: &Value| value.get("isError").and_then(Value::as_bool) == Some(true);
    output.get("error").is_some_and(|error| !error.is_null())
        || flagged(output)
        || output.get("result").is_some_and(flagged)
}

#[cfg(test)]
mod tests;
