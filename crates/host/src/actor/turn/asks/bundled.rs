//! Forced ask's one exception (decision record 0063, "Forced ask"): the host answers Clauseo's
//! own bundled tools itself, with their `allow_once` option and no card, so a lawyer in a
//! forced-ask matter (`agent/forced_ask.rs`) is not asked before every case-law search.
//!
//! The tools, by exact Claude name (`mcp__<server>__<tool>`), and nothing else:
//!
//! - `mcp__case-law__search_law`
//! - `mcp__case-law__read_document`
//! - `mcp__case-law__citator`
//! - `mcp__case-law__check_citations`
//! - `mcp__case-law__account`
//!
//! These are `agent/loads/case_law.rs` [`UNASKED_TOOLS`](crate::agent::loads::case_law::UNASKED_TOOLS),
//! the tools a Claude session outside forced ask pre-allows. `recharge`, which opens a payment for
//! a credit pack, and any other tool of the server still ask, as does anything else that spends
//! money or credits beyond the relay's own turn and day caps. Clauseo has no other bundled tool on
//! this tree that a forced-ask Claude session asks about: its document tools are no MCP server,
//! and Claude has no browser on any OS (`clauseo_profile::servers::no_browser`); a browser entry
//! for Claude would join here keyed on the host's own record of sending it, never on its name.
//!
//! Which server a tool belongs to comes only from what the host itself launched: the session's
//! header shows case law `Available` only when the host filled and sent its own `case-law` entry
//! this launch (`header_tools::sent`; `Unavailable`, `ManagedByFirm` under the firm's
//! `managed-mcp.json`, or nothing at all otherwise). The name alone never decides: `case-law` is
//! reserved (`clauseo_settings::reserved_mcp_server`), and a lawyer's server so named is dropped
//! before any launch, so with no entry of the host's the same tool name asks. The tool name is the
//! one the logged tool call carries (Claude Code's `mcp__<server>__<tool>`; no listed server name
//! holds `__`, `clauseo_settings::leaf::mcp`).
//!
//! The answer follows `codex_auto.rs`: nothing is logged on the thread (the tool call itself
//! still shows), and `tracing` gets the thread id and the tool's fixed name from the list above,
//! never its arguments (decision record 0047).

use clauseo_protocol::{AgentChoiceId, AgentKind, ClauseoTool, PermissionKind, StreamId, ThreadId};

use super::super::super::{Actor, header_tools};
use super::super::spawn;
use super::super::updates::agent;
use super::PermissionAsk;
use crate::agent::loads::case_law::{CLAUDE_SERVER, unasked_tool};
use crate::log_file::Cause;

/// Answers `ask` itself when it is one of the bundled tools ([`bundled_tool_to_allow`]); whether
/// it did. The caller then stops: the request is answered.
pub(super) fn answered(actor: &mut Actor, thread: ThreadId, ask: &PermissionAsk) -> bool {
    let Some((tool, option)) = bundled_tool_to_allow(actor, thread, ask) else {
        return false;
    };
    let Some(agent) = agent(actor, thread) else {
        return false;
    };
    let client = agent.link.client.clone();
    let key = ask.key;
    tracing::info!(%thread, tool, "allowed a bundled tool in forced ask");
    spawn(actor, async move {
        if let Err(error) = client.answer_permission(key, option).await {
            tracing::warn!(error = %Cause(&error), "a bundled tool's approval did not reach the agent");
        }
    });
    true
}

/// The bundled tool's case-law name and the `allow_once` option, when the host answers this
/// request itself: the thread's agent is Claude in forced ask (outside it, Claude's
/// `allowedTools` already lets these run); the request offers an option of kind `allow_once`;
/// the logged tool call it names is `mcp__case-law__<tool>` with `<tool>` one of
/// `UNASKED_TOOLS`, by exact name; and the host sent its own `case-law` entry this launch.
pub(super) fn bundled_tool_to_allow(
    actor: &mut Actor,
    thread: ThreadId,
    ask: &PermissionAsk,
) -> Option<(&'static str, AgentChoiceId)> {
    let kind = actor
        .check_thread(thread)
        .ok()
        .and_then(|entry| AgentKind::of(&entry.row.agent));
    if kind != Some(AgentKind::Claude) || !agent(actor, thread)?.forced_ask {
        return None;
    }
    let option = ask
        .options
        .iter()
        .find(|option| option.kind == PermissionKind::AllowOnce)?;
    let option = AgentChoiceId::new(&option.option_id).ok()?;
    let name = tool_name(actor, thread, ask)?;
    let tool = case_law_tool(&name)?;
    header_tools::sent(actor, thread, ClauseoTool::CaseLaw).then_some((tool, option))
}

/// The case-law tool `name` calls, as its entry in `UNASKED_TOOLS`: `mcp__case-law__<tool>`,
/// by exact string. `recharge`, another server, another case, or anything after the tool is
/// none.
pub(super) fn case_law_tool(name: &str) -> Option<&'static str> {
    let tool = name.strip_prefix(CLAUDE_SERVER)?.strip_prefix("__")?;
    if !unasked_tool(tool) {
        return None;
    }
    crate::agent::loads::case_law::UNASKED_TOOLS
        .into_iter()
        .find(|unasked| *unasked == tool)
}

/// The tool name of the call `ask` is about, as the thread's log (or the replay being loaded)
/// has it.
fn tool_name(actor: &mut Actor, thread: ThreadId, ask: &PermissionAsk) -> Option<String> {
    let (session, id) = (&ask.session, ask.acp_tool_call_id.as_str());
    let logged = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.sent.tool_call(session, id))
        .and_then(|(_, tool)| tool.tool_name.as_deref().map(str::to_owned));
    logged.or_else(|| {
        let replay = agent(actor, thread)?.replay.as_ref()?;
        let (_, tool) = replay.tool_call(session, id)?;
        tool.tool_name.as_deref().map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::case_law_tool;

    #[test]
    fn only_case_laws_unasked_tools_by_exact_name() {
        for tool in [
            "search_law",
            "read_document",
            "citator",
            "check_citations",
            "account",
        ] {
            assert_eq!(case_law_tool(&format!("mcp__case-law__{tool}")), Some(tool));
        }
        for name in [
            // Spends money: always asks.
            "mcp__case-law__recharge",
            "mcp__Case-Law__search_law",
            "mcp__case-law__Search_law",
            "mcp__case-law__search_law ",
            "mcp__case-law__search_law__x",
            "mcp__case-law_search_law",
            "mcp__case-law",
            "mcp__case-law-2__search_law",
            "mcp__case_law__search_law",
            "mcp__docs__search_law",
            "search_law",
            "WebFetch",
            "",
        ] {
            assert_eq!(case_law_tool(name), None, "{name}");
        }
    }
}
