//! The lawyer's MCP servers run without asking: adding a server to `mcp.servers` is the trust
//! (Rohan, 2026-11-11; `measurements/mcp-tools/plan.md`). A server is trusted when it is in the
//! effective list (the lawyer's, or the firm's policy list that replaces it), turned on, not one
//! of Clauseo's reserved names (`browser`, `case-law`, which keep their own rules), and Clauseo
//! controls MCP on this machine ([`McpControl::Clauseo`]; under the firm's `managed-mcp.json` no
//! listed server is sent, so none is trusted).
//!
//! - Claude: `allowedTools` gains `mcp__<name>` per trusted server ([`claude_rule`];
//!   `agent/loads/profile.rs`), the pattern of case law's pre-allow. Claude Code splits a tool
//!   name `mcp__<server>__<tool>` at `__` and a server-level rule matches that server's tools by
//!   its exact name, and no listed name holds `__` (`clauseo_settings::leaf::mcp`), so
//!   `mcp__case` never covers `case-law`'s tools.
//! - Codex: the host answers a trusted server's tool approval `allow_once` itself, once per
//!   call (`actor/turn/asks/codex_auto.rs`), only for a server Clauseo sent that session. The
//!   server comes only from what codex-acp 2.2.2 and Codex build, never from model-written text
//!   ([`codex_call`]), and is compared by exact string. A standalone request always asks.
//! - Forced ask (`agent/forced_ask.rs`) withholds the trust: Claude then gets no `allowedTools`
//!   at all, and the `mcp__*` ask rule; the host answers no Codex approval for it.

use std::collections::BTreeMap;

use clauseo_acp::ToolCall;
use clauseo_protocol::{CompactString, TextOrBlob, ToolCallPatch, ToolCallStatus};
use clauseo_settings::McpServerLaunch;
use serde_json::Value;

use crate::agent::loads::McpControl;

/// The servers whose tools run without asking, in name order: those of `servers` turned on and
/// not reserved, under [`McpControl::Clauseo`]; none under [`McpControl::Managed`].
pub(crate) fn trusted_servers(
    servers: &BTreeMap<String, McpServerLaunch>,
    control: McpControl,
) -> Vec<String> {
    match control {
        McpControl::Managed => Vec::new(),
        McpControl::Clauseo => servers
            .iter()
            .filter(|(name, server)| {
                server.enabled && clauseo_settings::reserved_mcp_server(name).is_none()
            })
            .map(|(name, _)| name.clone())
            .collect(),
    }
}

/// Whether `server` is one of `trusted`, by exact name, and not a reserved one (a check of its
/// own, should a reserved name ever reach `trusted`).
pub(crate) fn is_trusted(trusted: &[String], server: &str) -> bool {
    clauseo_settings::reserved_mcp_server(server).is_none()
        && trusted.iter().any(|name| name == server)
}

/// Whether `tool` of `server` is a built-in's that never asks: one of Clauseo's own `case-law`
/// connector's search and read tools (`loads::case_law::UNASKED_TOOLS`), by exact server and
/// tool name. Codex 0.162.1 asks before an MCP tool unless the tool says `readOnlyHint: true`
/// (`core/src/mcp_tool_call.rs:2461-2491`, the `Auto` mode codex-acp's servers run in), so a
/// tool whose hint is missing would ask; the host answers for these. `recharge`, which opens a
/// payment, and any other tool still ask. The
/// server name is reserved: no settings entry of it, in any case, is sent
/// (`clauseo_settings::reserved_mcp_server`), so a lawyer's or firm's server cannot borrow it.
/// The `browser`'s own rule is `sources::browser::auto_allowed_browser_tool`.
pub(crate) fn never_asks(server: &str, tool: Option<&str>) -> bool {
    server == clauseo_profile::servers::CASE_LAW
        && tool.is_some_and(crate::agent::loads::case_law::unasked_tool)
}

/// Claude's allow rule for every tool of `server`.
pub(crate) fn claude_rule(server: &str) -> String {
    format!("mcp__{server}")
}

/// The server and tool of a Codex MCP tool call, from its `tool_call` update: codex-acp 2.2.2
/// sends `rawInput` `{server, tool, arguments}` built from Codex's `mcpToolCall` item and the
/// title `` `mcp.${server}.${tool}` `` (`dist/index.js:29492-29522`, `McpToolReporter`). Both
/// must agree exactly. A shell command's title is its command text (`stripShellPrefix`,
/// `:28487`), so a title alone could be written by the model; its `rawInput` is
/// `{command, cwd}`, never a `server`.
pub(crate) fn codex_call<'a>(title: &str, raw_input: &'a Value) -> Option<(&'a str, &'a str)> {
    let server = raw_input.get("server")?.as_str()?;
    let tool = raw_input.get("tool")?.as_str()?;
    if server.is_empty() || tool.is_empty() || raw_input.get("command").is_some() {
        return None;
    }
    let expected = title
        .strip_prefix("mcp.")
        .and_then(|rest| rest.strip_prefix(server))
        .and_then(|rest| rest.strip_prefix('.'));
    (expected == Some(tool)).then_some((server, tool))
}

/// The most unfinished Codex MCP calls a thread keeps for [`CodexCalls`]; past it they are
/// dropped, so calls an agent never finished cannot pile up for the host's life.
const MAX_CODEX_CALLS: usize = 64;

/// A thread's Codex MCP tool calls that have not finished: the server and tool of each, by the
/// agent's tool call id, so a permission request that names only the call can be attributed.
/// Only a call [`codex_call`] reads is kept; a final status drops it.
#[derive(Debug, Default)]
pub(crate) struct CodexCalls(BTreeMap<CompactString, (CompactString, CompactString)>);

impl CodexCalls {
    /// A Codex tool call started: an MCP one is kept until it ends. codex-acp sends an MCP call
    /// with no `name` (`McpToolReporter`); a dynamic tool's carries one (`DynamicToolReporter`,
    /// `dist/index.js:28580-28591`), so a call with a name is never kept.
    pub(crate) fn started(&mut self, call: &ToolCall) {
        if call.tool_name.is_some() {
            return;
        }
        if matches!(
            call.status,
            ToolCallStatus::Completed | ToolCallStatus::Failed | ToolCallStatus::Cancelled
        ) {
            return;
        }
        let Some(TextOrBlob::Inline(text)) = &call.raw_input else {
            return;
        };
        let Ok(input) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let Some((server, tool)) = codex_call(&call.title, &input) else {
            return;
        };
        if self.0.len() >= MAX_CODEX_CALLS {
            self.0.clear();
        }
        let entry = (CompactString::from(server), CompactString::from(tool));
        self.0.insert(call.acp_id.clone(), entry);
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

    /// How many calls are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// The server and tool of the unfinished MCP call `acp_id`, taken out, if it is one.
    pub(crate) fn take(&mut self, acp_id: &str) -> Option<(String, String)> {
        self.0
            .remove(acp_id)
            .map(|(server, tool)| (server.to_string(), tool.to_string()))
    }

    /// The server and tool of the unfinished MCP call `acp_id`, if it is one.
    #[cfg(test)]
    pub(crate) fn call(&self, acp_id: &str) -> Option<(&str, &str)> {
        self.0
            .get(acp_id)
            .map(|(server, tool)| (server.as_str(), tool.as_str()))
    }
}

/// The option the host answers a trusted server's approval with: codex-acp 2.2.2's `allow_once`
/// (`McpApprovalOptionId.AllowOnce`, kind `allow_once`, `dist/index.js:32035-32041`), which runs
/// this one call. Never `allow_session` or `allow_always`: no lasting rule reaches the agent.
pub(crate) const ALLOW_ONCE: &str = "allow_once";

#[cfg(test)]
mod tests;
