//! The lawyer's notice for an MCP server that did not start
//! (`ProtocolWarning::McpServerNotStarted`: codex-acp's start-up report, and Claude's
//! failed-start notice, both mapped in `acp`): a warning of kind `tool_not_started` on the
//! conversation, which the app shows, once per server per session start, naming the tool as the
//! lawyer knows it. It goes through the same path as an
//! agent's own notice that names no prompt (`AgentUpdate::Notice`, origin `Idle`): held while
//! the session is prepared, then logged as it comes, with its kind, which puts it among the live
//! turn's entries, or outside any turn when none is live (`updates.rs`, `on_update`). The
//! host.log line is `warning.rs`'s.

use clauseo_acp::{AgentUpdate, Origin};
use clauseo_profile::servers::{BROWSER, CASE_LAW};
use clauseo_protocol::{AgentSessionId, NoticeKind, NoticeLevel, ThreadId};

use super::super::super::Actor;
use super::agent;

/// What the lawyer calls the case-law connector (its out-of-credit notice says "Case Law").
const CASE_LAW_NAME: &str = "Case Law";
/// What the lawyer calls the built-in browser, at the start of a sentence.
const BROWSER_NAME: &str = "The browser";

/// The update that tells the lawyer `server` did not start, the first time this session
/// reports it; `None` when this session told it already (an update for the same report, or a
/// second report of the same server), or when the thread has no agent.
pub(super) fn notice_once(
    actor: &mut Actor,
    thread: ThreadId,
    server: &str,
) -> Option<AgentUpdate> {
    let added = actor.effective.mcp_servers().keys().map(String::as_str);
    let text = text(&lawyer_name(server, added));
    let told = &mut agent(actor, thread)?.mcp_told;
    if !told.insert(server.to_owned()) {
        return None;
    }
    Some(AgentUpdate::Notice {
        level: NoticeLevel::Warning,
        text,
        kind: Some(NoticeKind::ToolNotStarted),
    })
}

/// The event for `update`, as an agent's update of `session` that came with no prompt named.
pub(super) fn event(session: AgentSessionId, update: AgentUpdate) -> clauseo_acp::AcpEvent {
    clauseo_acp::AcpEvent::Update {
        session,
        origin: Origin::Idle,
        update,
    }
}

/// The lawyer's name for `server` as the report names it: the built-ins' own names; for a tool
/// the lawyer added, the name they gave it (one of `added`, the settings' server names), in
/// double quotes. codex-acp turns each whitespace character of a name into `_` before Codex
/// sees it (`sanitizeMcpServerName`, `dist/index.js:42308-42311`), so the report may carry
/// that form; a name none of `added` matches goes in as it is.
fn lawyer_name<'a>(server: &str, mut added: impl Iterator<Item = &'a str>) -> String {
    match server {
        CASE_LAW => CASE_LAW_NAME.to_owned(),
        BROWSER => BROWSER_NAME.to_owned(),
        _ => {
            let given = added.find(|name| *name == server || codex_form(name) == server);
            format!("\"{}\"", given.unwrap_or(server))
        }
    }
}

/// `name` as codex-acp passes it on: each whitespace character as `_`.
fn codex_form(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_whitespace() { '_' } else { ch })
        .collect()
}

/// The notice's words: no error, no reason, no technical names.
fn text(name: &str) -> String {
    format!("{name} didn't start, so this answer runs without it. Ask again to try once more.")
}

#[cfg(test)]
mod tests {
    use super::{lawyer_name, text};

    #[test]
    fn the_built_ins_go_by_the_lawyer_s_names() {
        let none = std::iter::empty();
        assert_eq!(
            text(&lawyer_name("case-law", none)),
            "Case Law didn't start, so this answer runs without it. Ask again to try once more."
        );
        let none = std::iter::empty();
        assert_eq!(
            text(&lawyer_name("browser", none)),
            "The browser didn't start, so this answer runs without it. Ask again to try once \
             more."
        );
    }

    /// An added tool goes by the name the lawyer gave it, also when codex-acp reported its
    /// whitespace as `_`; a name the settings do not hold goes in as reported.
    #[test]
    fn an_added_tool_goes_by_the_name_the_lawyer_gave_it() {
        let added = ["Firm DMS", "westlaw"];
        assert_eq!(
            text(&lawyer_name("Firm_DMS", added.into_iter())),
            "\"Firm DMS\" didn't start, so this answer runs without it. Ask again to try once \
             more."
        );
        assert_eq!(lawyer_name("westlaw", added.into_iter()), "\"westlaw\"");
        assert_eq!(lawyer_name("fails", added.into_iter()), "\"fails\"");
    }
}
