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
//! result is an error (`isError`, Codex's `error`, or a failed status) records no source.
//! `account` and `recharge` record nothing. `Origin::Late` and `Origin::Replay` never reach
//! here (`actor/turn/updates.rs`), so a replay adds nothing.
//!
//! The step's charge: a `search_law`, `read_document`, `citator` or `check_citations` call
//! that completed or failed, whose entry carries the connector's `_meta` charge, also logs
//! `ToolCallCharged` ([`charge_of`]): the credits when more than 0, or `refused` when the
//! result was an error that charged 0. A success that charged 0 (a free repeat) logs nothing, as
//! does an entry with no charge (an older connector, a malformed one) and a call with no entry.
//! A call still running never gets one. Two results waiting under one key (the same tool with
//! the same arguments, in flight together) whose charges differ cannot be told apart: neither
//! call gets a charge ([`CaseLaw::take`]). A call cancelled, or failed with no result waiting,
//! may leave its result behind (or have it come back later): the results under its key then show
//! no charge either ([`CaseLaw::let_go`]), so a later identical call never takes its number.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    AgentId, CompactString, DomainEvent, MAX_SEARCH_QUERY_CHARS, PageVia, PageVisited,
    SearchSource, ToolCallCharged, ToolCallId, ToolCallPatch, ToolCallStatus, TurnId, WebResult,
    WebSearchRecorded,
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
    /// Calls that ended with no result taken (cancelled, or failed before theirs came back):
    /// the next result under their key may be theirs, so it shows no charge.
    orphans: VecDeque<Orphan>,
}

/// A call that ended with no result taken, by its key.
#[derive(Debug)]
struct Orphan {
    at: Duration,
    tool: CaseLawTool,
    key: [u8; 32],
}

#[derive(Debug)]
struct Entry {
    at: Duration,
    found: CaseLawFound,
    /// Another entry under its key carried a different charge: its charge is not shown.
    ambiguous: bool,
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
        // A call of this key ended with nothing taken: this result may be its, come back late.
        let orphan = self
            .orphans
            .iter()
            .position(|orphan| orphan.tool == found.tool && orphan.key == found.key);
        let ambiguous = orphan.is_some();
        if let Some(index) = orphan {
            self.orphans.remove(index);
        }
        self.entries.push_back(Entry {
            at: now,
            found,
            ambiguous,
        });
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
            ToolCallStatus::Failed => self.failed(now, &pending),
            ToolCallStatus::Cancelled => {
                self.let_go(now, &pending);
                Vec::new()
            }
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
            Some(ToolCallStatus::Failed) => match self.calls.remove(acp_id) {
                Some(pending) => self.failed(now, &pending),
                None => Vec::new(),
            },
            Some(ToolCallStatus::Cancelled) => {
                if let Some(pending) = self.calls.remove(acp_id) {
                    self.let_go(now, &pending);
                }
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
        self.orphans.clear();
    }

    /// Drops the results older than [`ENTRY_LIFE`].
    fn expire(&mut self, now: Duration) {
        self.entries
            .retain(|entry| now.saturating_sub(entry.at) <= ENTRY_LIFE);
        self.orphans
            .retain(|orphan| now.saturating_sub(orphan.at) <= ENTRY_LIFE);
    }

    /// A call ended with no result taken (cancelled, or failed with none waiting): a result
    /// waiting under its key may be its, and so may the next to come, so neither shows a
    /// charge. Each later call still takes the oldest result under the key, and the charge
    /// check in [`CaseLaw::take`] blanks any that could be another's.
    fn let_go(&mut self, now: Duration, call: &Pending) {
        self.expire(now);
        let key = join_key(call.tool, &arguments_of(call));
        let mut waiting = false;
        for entry in &mut self.entries {
            if entry.found.tool == call.tool && entry.found.key == key {
                entry.ambiguous = true;
                waiting = true;
            }
        }
        if waiting {
            return;
        }
        if self.orphans.len() >= MAX_ENTRIES {
            self.orphans.pop_front();
        }
        self.orphans.push_back(Orphan {
            at: now,
            tool: call.tool,
            key,
        });
    }

    /// Takes the entry for `tool` with `key`, if one is young enough. When another entry waits
    /// under the same key with a different charge (or one taken before it was marked so), the
    /// two calls cannot be told apart: the taken entry's charge is dropped and the others are
    /// marked, so their calls show none either. The links stay: they are only sources.
    fn take(&mut self, now: Duration, tool: CaseLawTool, key: &[u8; 32]) -> Option<CaseLawFound> {
        self.expire(now);
        let same = |entry: &Entry| entry.found.tool == tool && entry.found.key == *key;
        let index = self.entries.iter().position(same)?;
        let mut taken = self.entries.remove(index)?;
        let charge = |entry: &Entry| (entry.found.charged, entry.found.failed);
        let differs = self
            .entries
            .iter()
            .any(|other| same(other) && charge(other) != charge(&taken));
        if differs {
            for other in self.entries.iter_mut().filter(|entry| same(entry)) {
                other.ambiguous = true;
            }
        }
        if differs || taken.ambiguous {
            tracing::debug!(
                tool = tool.name(),
                "two case-law results under one key differ in charge; neither shows one"
            );
            taken.found.charged = None;
        }
        Some(taken.found)
    }

    /// The record a call whose agent said it failed adds: its charge only.
    fn failed(&mut self, now: Duration, call: &Pending) -> Vec<DomainEvent> {
        let arguments = arguments_of(call);
        let Some(found) = self.take(now, call.tool, &join_key(call.tool, &arguments)) else {
            self.let_go(now, call);
            return Vec::new();
        };
        charge_of(call, &found).into_iter().collect()
    }

    /// The records a completed call adds: its sources, then its charge.
    fn finished(&mut self, now: Duration, agent: &AgentId, call: &Pending) -> Vec<DomainEvent> {
        let arguments = arguments_of(call);
        let entry = self.take(now, call.tool, &join_key(call.tool, &arguments));
        let charge = entry.as_ref().and_then(|found| charge_of(call, found));
        let mut records = sources(agent, call, &arguments, entry);
        records.extend(charge);
        records
    }
}

/// The sources a completed call adds, from its entry if any.
fn sources(
    agent: &AgentId,
    call: &Pending,
    arguments: &Value,
    entry: Option<CaseLawFound>,
) -> Vec<DomainEvent> {
    let failed = entry.as_ref().is_some_and(|found| found.failed);
    if failed || call.output.as_ref().is_some_and(is_error) {
        tracing::debug!(tool_call = %call.tool_call, "a case-law call that failed is not recorded");
        return Vec::new();
    }
    let links = entry.map(|found| found.links).unwrap_or_default();
    match call.tool {
        CaseLawTool::SearchLaw => {
            let Some(query) = query_of(arguments) else {
                return Vec::new();
            };
            let results = links.iter().filter_map(result_of).collect();
            let record =
                WebSearchRecorded::new(call.turn, call.tool_call, agent.clone(), &query, results)
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
        CaseLawTool::Citator | CaseLawTool::CheckCitations => Vec::new(),
    }
}

/// The charge a call's entry gives its step: the credits when more than 0; `refused` when the
/// result was an error that charged 0; nothing for a success that charged 0 or an entry with no
/// charge.
fn charge_of(call: &Pending, found: &CaseLawFound) -> Option<DomainEvent> {
    let credits = found.charged?;
    let refused = found.failed && credits == 0;
    (credits > 0 || refused).then(|| {
        ToolCallCharged {
            turn: call.turn,
            tool_call: call.tool_call,
            credits,
            refused,
        }
        .into()
    })
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
