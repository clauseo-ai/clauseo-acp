//! Claude's report of an MCP server that did not start: a `notice`, which becomes a warning per
//! server instead, as codex-acp's report does (`mcp_startup.rs`), so the host words it.
//!
//! claude-agent-acp 0.112.0 (`dist/session-notices.js`, `mcpServerStartupNotice` and
//! `mcpServerStartupNotices`) sends a `warning` notice with no `_meta`: for one server, the title
//! `MCP server "<name>" failed to start` and the description `Its tools are unavailable in this
//! session. Run /mcp for the error.` or `Its tools are unavailable in this session. <error>`; for
//! several servers that share one error, the title `<n> MCP servers failed to start` and the
//! description `The tools of "a", "b" and "c" are unavailable in this session. <error>`. Its
//! notice that a server needs authentication (`MCP server "<name>" needs authentication`) stays
//! a notice.

use agent_client_protocol::schema::v1 as acp;

use crate::event::{McpStartupReason, ProtocolWarning};

/// The title for one server is `MCP server "<name>" failed to start`.
const ONE_PREFIX: &str = "MCP server \"";
const ONE_SUFFIX: &str = "\" failed to start";
/// The title for several is `<n> MCP servers failed to start`.
const MANY_SUFFIX: &str = " MCP servers failed to start";
/// Their description names them: `The tools of "a", "b" and "c" are unavailable …`.
const MANY_PREFIX: &str = "The tools of ";
const MANY_END: &str = " are unavailable in this session";

/// One warning per server `notice` reports as not started, in the order it names them; `None`
/// for any other notice, which stays a notice.
pub(crate) fn failed(notice: &acp::Notice) -> Option<Vec<ProtocolWarning>> {
    let servers = servers(&notice.title, notice.description.as_deref())?;
    let warning = |server| ProtocolWarning::McpServerNotStarted {
        server,
        reason: McpStartupReason::Failed,
    };
    Some(servers.into_iter().map(warning).collect())
}

/// The servers a notice titled `title`, described as `description`, reports as not started.
fn servers(title: &str, description: Option<&str>) -> Option<Vec<String>> {
    if let Some(name) = title
        .strip_prefix(ONE_PREFIX)
        .and_then(|rest| rest.strip_suffix(ONE_SUFFIX))
    {
        return (!name.is_empty()).then(|| vec![name.to_owned()]);
    }
    let count: usize = title.strip_suffix(MANY_SUFFIX)?.parse().ok()?;
    let list = description?
        .strip_prefix(MANY_PREFIX)?
        .split_once(MANY_END)?
        .0;
    let names = names(list)?;
    (count >= 2 && names.len() == count).then_some(names)
}

/// The names of a list `"a", "b" and "c"` (or `"a" and "b"`); `None` when it is not of that
/// shape or a name is empty.
fn names(list: &str) -> Option<Vec<String>> {
    let inner = list.strip_prefix('"')?.strip_suffix('"')?;
    let (head, last) = inner.rsplit_once("\" and \"")?;
    let names: Vec<String> = head
        .split("\", \"")
        .chain([last])
        .map(str::to_owned)
        .collect();
    names.iter().all(|name| !name.is_empty()).then_some(names)
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1 as acp;

    use super::failed;
    use crate::event::{McpStartupReason, ProtocolWarning};

    fn notice(title: &str, description: &str) -> acp::Notice {
        acp::Notice::new(acp::NoticeSeverity::Warning, title).description(description.to_owned())
    }

    fn not_started(servers: &[&str]) -> Vec<ProtocolWarning> {
        let warning = |server: &&str| ProtocolWarning::McpServerNotStarted {
            server: (*server).to_owned(),
            reason: McpStartupReason::Failed,
        };
        servers.iter().map(warning).collect()
    }

    #[test]
    fn one_server() {
        let one = notice(
            "MCP server \"firm-precedents\" failed to start",
            "Its tools are unavailable in this session. Run /mcp for the error.",
        );
        assert_eq!(failed(&one), Some(not_started(&["firm-precedents"])));
    }

    #[test]
    fn a_name_with_spaces() {
        let one = notice(
            "MCP server \"Firm DMS\" failed to start",
            "Its tools are unavailable in this session. connect ECONNREFUSED 127.0.0.1:9",
        );
        assert_eq!(failed(&one), Some(not_started(&["Firm DMS"])));
    }

    #[test]
    fn several_servers_with_one_error() {
        let three = notice(
            "3 MCP servers failed to start",
            "The tools of \"a\", \"Firm DMS\" and \"c\" are unavailable in this session. \
             spawn ENOENT",
        );
        assert_eq!(failed(&three), Some(not_started(&["a", "Firm DMS", "c"])));
        let two = notice(
            "2 MCP servers failed to start",
            "The tools of \"a\" and \"b\" are unavailable in this session. spawn ENOENT",
        );
        assert_eq!(failed(&two), Some(not_started(&["a", "b"])));
    }

    /// A count the description does not match stays a notice.
    #[test]
    fn a_count_that_does_not_match_stays_a_notice() {
        let wrong = notice(
            "3 MCP servers failed to start",
            "The tools of \"a\" and \"b\" are unavailable in this session. spawn ENOENT",
        );
        assert_eq!(failed(&wrong), None);
    }

    #[test]
    fn a_server_that_needs_authentication_stays_a_notice() {
        let auth = notice(
            "MCP server \"westlaw\" needs authentication",
            "Run /mcp to sign in.",
        );
        assert_eq!(failed(&auth), None);
    }

    #[test]
    fn another_notice_stays_a_notice() {
        let other = notice("Context compacted", "Older messages were summarised.");
        assert_eq!(failed(&other), None);
        let bare = acp::Notice::new(acp::NoticeSeverity::Info, "MCP servers failed to start");
        assert_eq!(failed(&bare), None);
    }
}
