//! The Codex approvals the host answers itself, with nothing logged: a guarded `browser` tool
//! (`allow_session`; the guard refuses eval), and a tool of a server the lawyer added
//! (`allow_once`, `agent/mcp_trust.rs`).

use clauseo_acp::PendingKey;
use clauseo_protocol::{AgentChoiceId, AgentKind, PermissionKind, ThreadId};

use super::super::super::Actor;
use super::super::spawn;
use super::super::updates::agent;
use super::PermissionAsk;
use crate::log_file::Cause;

/// The option codex-acp 2.2.2 offers to allow an MCP tool for the session, kind `allow_always`
/// (`dist/index.js:32663-32708`); answered, Codex gets `ApprovedForSession` and, in its default
/// `Auto` approval mode, asks no more for that server and tool in the session (codex rust-v0.162.1
/// `core/src/mcp_tool_call.rs:1730-1751`, `:2126-2163`; codex-acp `dist/index.js:32733-32750`).
const ALLOW_SESSION: &str = "allow_session";

/// The `browser` tool and the `allow_session` option, when the host answers this request itself:
/// the thread's agent is Codex, neither in forced ask nor unprotected (a failed folder check,
/// `agents.rs`: the lawyer gets the card then); the request offers `allow_session`; and it names
/// a guarded `browser` tool (`sources::browser::auto_allowed_browser_tool`; the guard refuses
/// eval). The tool comes only from fields Codex and codex-acp build, never model-written text:
/// the title `mcp.browser.<tool>` of the earlier tool call the request's toolCallId names (the
/// thread's `sources::browser::CodexTools`), or a standalone request's `serverName` and exact
/// message (`sources::browser::codex_standalone_browser_tool`). Anything else (shell commands,
/// file writes, other servers such as `case-law`) goes to the lawyer as before.
///
/// Codex's own per-server `default_tools_approval_mode` would do this, but codex-acp replaces the
/// whole `mcp_servers` table with ACP's servers (`dist/index.js:35482`), dropping that value.
pub(super) fn browser_tool_to_allow(
    actor: &mut Actor,
    thread: ThreadId,
    ask: &PermissionAsk,
    raw_input: Option<&serde_json::Value>,
) -> Option<(String, AgentChoiceId)> {
    let kind = actor
        .check_thread(thread)
        .ok()
        .and_then(|entry| AgentKind::of(&entry.row.agent));
    let skip = agent(actor, thread).is_none_or(|agent| agent.forced_ask || agent.unprotected());
    if kind != Some(AgentKind::Codex) || skip {
        return None;
    }
    let option = ask
        .options
        .iter()
        .find(|option| option.option_id == ALLOW_SESSION)?;
    let option = AgentChoiceId::new(&option.option_id).ok()?;
    let standalone =
        crate::sources::browser::codex_standalone_browser_tool(&ask.acp_tool_call_id, raw_input);
    let tool = match standalone {
        Some(tool) => tool.to_owned(),
        None => super::super::state(actor, thread)?
            .sources
            .codex_browser_tool(&ask.acp_tool_call_id)?
            .to_owned(),
    };
    crate::sources::browser::auto_allowed_browser_tool(&tool).then_some((tool, option))
}

/// Answers a Codex `browser` tool's approval with `option` (`allow_session`), with nothing
/// logged on the thread: the lawyer never sees it. `tracing` gets the tool's name, never its
/// arguments.
pub(super) fn allow_browser_tool(
    actor: &mut Actor,
    thread: ThreadId,
    key: PendingKey,
    tool: &str,
    option: AgentChoiceId,
) {
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    let client = agent.link.client.clone();
    tracing::info!(%thread, tool, "allowed a browser tool for the session");
    spawn(actor, async move {
        if let Err(error) = client.answer_permission(key, option).await {
            tracing::warn!(error = %Cause(&error), "a browser tool's approval did not reach the agent");
        }
    });
}

/// The server and the `allow_once` option, when the host answers this request itself because
/// the lawyer added the server (`agent/mcp_trust.rs`): the thread's agent is Codex, not in
/// forced ask; the request offers `allow_once` of kind `allow_once`; and it belongs to a
/// server that is trusted now (turned on in the effective `mcp.servers`, not reserved, and
/// Clauseo controls MCP), by exact name. The server comes only from what codex-acp and Codex
/// build: a standalone request's id and `serverName`
/// (`mcp_trust::codex_standalone_server`), or the earlier MCP tool call the request's
/// toolCallId names, whose title and `rawInput` agree (`mcp_trust::CodexCalls`). A shell
/// command, a file change and a server's own question (`accept`/`decline`, no `allow_once`)
/// go on as before. Clauseo's own `case-law` server is answered the same way
/// (`mcp_trust::never_asks`): it only searches, and Codex asks for its tools without
/// `readOnlyHint` (`recharge`). The `browser` keeps its own rule above.
pub(super) fn trusted_tool_to_allow(
    actor: &mut Actor,
    thread: ThreadId,
    ask: &PermissionAsk,
    raw_input: Option<&serde_json::Value>,
) -> Option<(String, AgentChoiceId)> {
    let kind = actor
        .check_thread(thread)
        .ok()
        .and_then(|entry| AgentKind::of(&entry.row.agent));
    if kind != Some(AgentKind::Codex) || agent(actor, thread).is_none_or(|agent| agent.forced_ask) {
        return None;
    }
    let option = ask.options.iter().find(|option| {
        option.option_id == crate::agent::mcp_trust::ALLOW_ONCE
            && option.kind == PermissionKind::AllowOnce
    })?;
    let option = AgentChoiceId::new(&option.option_id).ok()?;
    let standalone = crate::agent::mcp_trust::codex_standalone_server(
        ask.session.as_str(),
        &ask.acp_tool_call_id,
        raw_input,
    );
    let server = match standalone {
        Some(server) => server.to_owned(),
        None => super::super::state(actor, thread)?
            .sources
            .codex_mcp_server(&ask.acp_tool_call_id)?
            .to_owned(),
    };
    if crate::agent::mcp_trust::never_asks(&server) {
        return Some((server, option));
    }
    let control = crate::agent::loads::mcp_control(&actor.managed_mcp);
    let trusted =
        crate::agent::mcp_trust::trusted_servers(&actor.effective.settings().mcp.servers, control);
    crate::agent::mcp_trust::is_trusted(&trusted, &server).then_some((server, option))
}

/// Answers a trusted server's tool approval with `option` (`allow_once`), with nothing logged
/// on the thread: the lawyer added the server, and the tool call itself still shows. `tracing`
/// gets the server's name, never the tool's arguments.
pub(super) fn allow_trusted_tool(
    actor: &mut Actor,
    thread: ThreadId,
    key: PendingKey,
    server: &str,
    option: AgentChoiceId,
) {
    let Some(agent) = agent(actor, thread) else {
        return;
    };
    let client = agent.link.client.clone();
    tracing::info!(%thread, server, "allowed a tool of a trusted server");
    spawn(actor, async move {
        if let Err(error) = client.answer_permission(key, option).await {
            tracing::warn!(error = %Cause(&error), "a trusted tool's approval did not reach the agent");
        }
    });
}
