//! `PageVisited { via: Browser }`: one per browser open that completed (S27, "The record";
//! decision record 0029, "`PageVisited`").
//!
//! The open tool is looked up by name, as each agent reports it:
//! - Claude: [`CLAUDE_OPEN`], `mcp__<server>__<tool>`, in the tool call's `name` or `title`;
//! - Codex: [`CODEX_OPEN`]. codex-acp 2.2.2 titles an MCP call `` `mcp.${server}.${tool}` ``
//!   and sends no `name` (`dist/index.js:29492-29503`); its `rawInput` is
//!   `{ server, tool, arguments }` (`:29520-29522`), and the completed update carries the MCP
//!   result as `rawOutput` `{ result, error }` (`:29507-29513`, `:29524`), with no `content`.
//!
//! The URL is the `url` of the last `rawInput` the host had for the call (at its top level, or
//! under Codex's `arguments`), taken when an update reports it `completed` in the live turn; a
//! value with no `:` gets `https://` as agent-browser gives it, and only `http` and `https`
//! URLs count. The title is the tool result's text, which agent-browser makes the page's title
//! for an open (`cli/src/mcp.rs`, `response_text`), when there is one and it is not the URL,
//! without control or bidi formatting characters. A failed, cancelled or unfinished call writes
//! nothing, and so does a completed one whose result is an error (`isError`, Codex's `error`,
//! or the guard's "Clauseo does not allow" refusal), and so does an update the host does not log in
//! the live turn: `Origin::Late` and `Origin::Replay` never reach here (`actor/turn/updates.rs`).

use std::collections::BTreeMap;

use clauseo_acp::ToolCall;
use clauseo_protocol::{
    CompactString, PageVia, PageVisited, TextOrBlob, ToolCallContent, ToolCallId, ToolCallPatch,
    ToolCallStatus, TurnId,
};
use serde_json::Value;

/// agent-browser's open tool as Claude names it: server `browser`, tool `agent_browser_open`.
pub(crate) const CLAUDE_OPEN: &str = "mcp__browser__agent_browser_open";

/// The same tool as codex-acp 2.2.2 titles it (`dist/index.js:29492-29503`).
pub(crate) const CODEX_OPEN: &str = "mcp.browser.agent_browser_open";

/// The most unfinished opens a thread keeps; past it they are dropped, so opens an agent never
/// finished cannot pile up for the host's life.
const MAX_OPENS: usize = 64;

/// A thread's browser opens that have not finished, by the agent's tool call id. Kept across
/// turns and agent restarts; only completions in the live turn are recorded.
#[derive(Debug, Default)]
pub(crate) struct Opens(BTreeMap<CompactString, Open>);

/// One open: the host's id for the call and the URL its last input named.
#[derive(Debug)]
struct Open {
    tool_call: ToolCallId,
    url: Option<String>,
}

impl Opens {
    /// A tool call started in `turn` as `tool_call`: an open is followed until it ends, or
    /// recorded at once when it started completed.
    pub(crate) fn started(
        &mut self,
        turn: TurnId,
        tool_call: ToolCallId,
        call: &ToolCall,
    ) -> Option<PageVisited> {
        if !is_open(&call.title, call.tool_name.as_deref()) {
            return None;
        }
        let open = Open {
            tool_call,
            url: call.raw_input.as_ref().and_then(url_of),
        };
        match call.status {
            ToolCallStatus::Completed => {
                visited(turn, open, &call.content, call.raw_output.as_ref())
            }
            ToolCallStatus::Failed | ToolCallStatus::Cancelled => None,
            ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => {
                if self.0.len() >= MAX_OPENS {
                    // Opens that never finished (an agent that died mid-call) are let go.
                    self.0.clear();
                }
                self.0.insert(call.acp_id.clone(), open);
                None
            }
        }
    }

    /// An update of the call the agent knows as `acp_id`, logged in `turn`: its input replaces
    /// the URL; `completed` records the page, `failed` or `cancelled` forgets the call.
    pub(crate) fn updated(
        &mut self,
        turn: TurnId,
        acp_id: &str,
        patch: &ToolCallPatch,
    ) -> Option<PageVisited> {
        let open = self.0.get_mut(acp_id)?;
        if let Some(input) = &patch.raw_input {
            // The last input decides, even when it names no web page.
            open.url = url_of(input);
        }
        match patch.status? {
            ToolCallStatus::Completed => {
                let open = self.0.remove(acp_id)?;
                let content = patch.content.as_deref().unwrap_or_default();
                visited(turn, open, content, patch.raw_output.as_ref())
            }
            ToolCallStatus::Failed | ToolCallStatus::Cancelled => {
                self.0.remove(acp_id);
                None
            }
            ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => None,
        }
    }
}

/// The most unfinished Codex `browser` calls a thread keeps for [`CodexTools`]; past it they
/// are dropped, as [`MAX_OPENS`].
const MAX_CODEX_TOOLS: usize = 64;

/// A thread's Codex calls of the guarded `browser` server's tools that have not finished: the
/// tool each names, by the agent's tool call id, so a permission request that names only the
/// call (`actor/turn/asks.rs`) can be told apart. Only a call whose title is exactly
/// `mcp.browser.<tool>` as codex-acp 2.2.2 builds it (`dist/index.js:29492-29503`) is kept;
/// a final status drops it.
#[derive(Debug, Default)]
pub(crate) struct CodexTools(BTreeMap<CompactString, CompactString>);

impl CodexTools {
    /// A Codex tool call started: one of the `browser` server's is kept until it ends.
    pub(crate) fn started(&mut self, call: &ToolCall) {
        let Some(tool) = codex_browser_tool(&call.title) else {
            return;
        };
        match call.status {
            ToolCallStatus::Completed | ToolCallStatus::Failed | ToolCallStatus::Cancelled => {}
            ToolCallStatus::Pending | ToolCallStatus::InProgress | ToolCallStatus::Unknown => {
                if self.0.len() >= MAX_CODEX_TOOLS {
                    self.0.clear();
                }
                self.0
                    .insert(call.acp_id.clone(), CompactString::from(tool));
            }
        }
    }

    /// An update of the call the agent knows as `acp_id`: a final status forgets it.
    pub(crate) fn updated(&mut self, acp_id: &str, patch: &ToolCallPatch) {
        if matches!(
            patch.status,
            Some(ToolCallStatus::Completed | ToolCallStatus::Failed | ToolCallStatus::Cancelled)
        ) {
            self.0.remove(acp_id);
        }
    }

    /// The `browser` tool the unfinished call `acp_id` runs, if it is one.
    pub(crate) fn tool(&self, acp_id: &str) -> Option<&str> {
        self.0.get(acp_id).map(CompactString::as_str)
    }
}

/// The tool of a Codex call titled exactly `mcp.browser.<tool>` (codex-acp 2.2.2 titles an MCP
/// call `` `mcp.${server}.${tool}` ``, `dist/index.js:29492-29503`), `browser` being
/// [`clauseo_profile::servers::BROWSER`], a reserved name no settings entry may take.
pub(crate) fn codex_browser_tool(title: &str) -> Option<&str> {
    let tool = title
        .strip_prefix("mcp.")?
        .strip_prefix(clauseo_profile::servers::BROWSER)?
        .strip_prefix('.')?;
    (!tool.is_empty()).then_some(tool)
}

/// The tool a Codex standalone MCP approval names for the `browser` server: codex-acp 2.2.2
/// sends one with no earlier tool call as toolCallId `elicitation:<session>:<server>:<seq>` and
/// `rawInput` `{serverName, description: <message>, ...}` (`dist/index.js:32611-32626`); the
/// message is Codex's fallback `Allow the <server> MCP server to run tool "<tool>"?` (codex
/// rust-v0.162.1 `core/src/mcp_tool_call.rs:1956-1972`). A rendered template can replace that
/// message, and any text that is not exactly it names no tool.
pub(crate) fn codex_standalone_browser_tool<'a>(
    acp_tool_call_id: &str,
    raw_input: Option<&'a Value>,
) -> Option<&'a str> {
    let server = clauseo_profile::servers::BROWSER;
    let input = raw_input?;
    if !acp_tool_call_id.starts_with("elicitation:") || input.get("serverName")?.as_str()? != server
    {
        return None;
    }
    let message = input.get("description")?.as_str()?;
    let tool = message
        .strip_prefix("Allow the ")?
        .strip_prefix(server)?
        .strip_prefix(" MCP server to run tool \"")?
        .strip_suffix("\"?")?;
    (!tool.is_empty() && !tool.contains('"')).then_some(tool)
}

/// Whether the host answers a Codex approval of the `browser` server's `tool` itself: an
/// `agent_browser_*` name (ASCII lowercase, digits and `_`). The guard refuses every tool off
/// its list whatever the answer (`clauseo_mcp`'s `browser_guard/filter.rs` `ALLOWED_TOOLS`:
/// agent-browser's core tools less `agent_browser_eval`), and checks each call's arguments.
pub(crate) fn auto_allowed_browser_tool(tool: &str) -> bool {
    tool.strip_prefix("agent_browser_").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    })
}

/// Whether a tool call is the browser's open, by its name or, failing that, its title.
fn is_open(title: &str, tool_name: Option<&str>) -> bool {
    [Some(title), tool_name]
        .into_iter()
        .flatten()
        .any(|name| name == CLAUDE_OPEN || name == CODEX_OPEN)
}

/// The page an open that completed visited, when its input named a URL.
fn visited(
    turn: TurnId,
    open: Open,
    content: &[ToolCallContent],
    raw_output: Option<&TextOrBlob>,
) -> Option<PageVisited> {
    let Some(url) = open.url else {
        tracing::debug!(tool_call = %open.tool_call, "a browser open without a web url is not recorded");
        return None;
    };
    let text = result_text(content, raw_output);
    if is_error(raw_output)
        || text
            .as_deref()
            .is_some_and(|text| text.starts_with(REFUSED))
    {
        tracing::debug!(tool_call = %open.tool_call, "a browser open that failed is not recorded");
        return None;
    }
    let title = text
        .as_deref()
        .and_then(title_of)
        .filter(|title| *title != url);
    Some(PageVisited::new(
        turn,
        open.tool_call,
        &url,
        title,
        PageVia::Browser,
    ))
}

/// How the guard begins its answer to a call it refused (`crates/mcp`, record 0029).
const REFUSED: &str = "Clauseo does not allow";

/// The `url` of a tool input: at its top level (Claude), or under `arguments` (Codex), as a web
/// URL ([`web_url`]). An input held as a blob is too large to be an open's and is passed over.
fn url_of(raw_input: &TextOrBlob) -> Option<String> {
    let TextOrBlob::Inline(text) = raw_input else {
        return None;
    };
    let input: Value = serde_json::from_str(text).ok()?;
    let url = |value: &Value| value.get("url")?.as_str().map(str::to_owned);
    url(&input)
        .or_else(|| input.get("arguments").and_then(url))
        .and_then(|url| web_url(&url))
}

/// The page an open's `url` names, as agent-browser reads it: a value with no `:` gets
/// `https://` in front (`sci.gov.in/x`); then only an `http` or `https` URL counts, so
/// `about:blank` and other schemes are no page.
pub(crate) fn web_url(url: &str) -> Option<String> {
    let url = url.trim();
    let url = if url.contains(':') {
        url.to_owned()
    } else {
        format!("https://{url}")
    };
    let (scheme, rest) = url.split_once("://")?;
    let web = scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https");
    (web && !rest.is_empty() && super::is_safe_url_text(&url)).then_some(url)
}

/// Whether the MCP result in `rawOutput` says the call failed: a non-null `error` (Codex), or
/// `isError: true` at its top level or under `result`.
fn is_error(raw_output: Option<&TextOrBlob>) -> bool {
    let Some(TextOrBlob::Inline(raw)) = raw_output else {
        return false;
    };
    let Ok(output) = serde_json::from_str::<Value>(raw) else {
        return false;
    };
    let flagged = |value: &Value| value.get("isError").and_then(Value::as_bool) == Some(true);
    output.get("error").is_some_and(|error| !error.is_null())
        || flagged(&output)
        || output.get("result").is_some_and(flagged)
}

/// The tool result's text: its first text content (Claude), else the first text of the MCP result
/// in `rawOutput` (`content`, or Codex's `result.content`).
fn result_text(content: &[ToolCallContent], raw_output: Option<&TextOrBlob>) -> Option<String> {
    let from_content = content.iter().find_map(|item| {
        if let ToolCallContent::Text {
            text: TextOrBlob::Inline(text),
        } = item
        {
            Some(text.clone())
        } else {
            None
        }
    });
    from_content.or_else(|| {
        let TextOrBlob::Inline(raw) = raw_output? else {
            return None;
        };
        let output: Value = serde_json::from_str(raw).ok()?;
        let result = output.get("result").unwrap_or(&output);
        result
            .get("content")?
            .as_array()?
            .iter()
            .find_map(|item| item.get("text")?.as_str().map(str::to_owned))
    })
}

/// A page title from the result's text: its first line, without control characters or the
/// bidi formatting characters (U+202A to U+202E, U+2066 to U+2069) a page could spoof the
/// sources list with, trimmed; none when nothing is left.
fn title_of(text: &str) -> Option<String> {
    let line: String = text
        .lines()
        .next()?
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    let line = line.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

#[cfg(test)]
mod tests;
