//! A tool's rule and offer: an MCP tool's exact name and its parts, and what its card shows.

use clauseo_protocol::{AlwaysKind, AlwaysOffer};

/// A tool's offer: its server and its name there, never the whole `mcp__<server>__<tool>`, so
/// the card can say "{tool} from {server}". Web search has no server.
pub(super) fn tool_offer(server: Option<&str>, tool: &str) -> AlwaysOffer {
    AlwaysOffer {
        server: server.map(str::to_owned),
        tool: Some(tool.to_owned()),
        ..super::offer(AlwaysKind::Tool, None, None)
    }
}

/// An MCP tool's server and its name there: `mcp__<server>__<tool>` split at the first `__`
/// after the server's name (Claude Code's own form).
pub(super) fn mcp_parts(name: &str) -> Option<(&str, &str)> {
    name.strip_prefix("mcp__")?.split_once("__")
}

/// An MCP tool's exact name, `mcp__<server>__<tool>`: letters, digits, `_` and `-` only.
pub(super) fn is_mcp_tool(name: &str) -> bool {
    name.strip_prefix("mcp__").is_some_and(|rest| {
        rest.split_once("__")
            .is_some_and(|(server, tool)| !server.is_empty() && !tool.is_empty())
            && rest
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    })
}
