//! Claude's browser switch and the `browser` entry's allow rule, for `profile.rs`'s rules.

/// How a Claude session reaches the browser, for its rules. One value, so the shell rules and the
/// entry's allow rule never go together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudeBrowser {
    /// No browser: any hook rule naming agent-browser or the `browser` server is dropped, so a
    /// session without the browser has none.
    None,
    /// The browser as a shell tool (dormant: no session has it for launch,
    /// `clauseo_profile::servers::shell_browser`): `allowedTools` gains agent-browser's page
    /// subcommands, `disallowedTools` its denied flags, subcommands and file or JavaScript
    /// commands, and `permissions.ask` the options that still ask
    /// (`clauseo_profile::claude::agent_browser_allowed`, `agent_browser_denied`,
    /// `agent_browser_ask`).
    Shell,
    /// The guarded `browser` MCP entry (`clauseo_profile::servers::browser_mcp`):
    /// `allowedTools` gains the server-level rule `mcp__browser` (`mcp_trust::claude_rule`), so
    /// ordinary browsing never asks; the guard is what limits the tools. Not in forced ask, where
    /// there is no `allowedTools` and the `mcp__*` ask rule gives cards.
    Entry,
}

impl ClaudeBrowser {
    /// The entry when it went (`entry`), else the shell tool when the session has it (`shell`),
    /// else none.
    pub(crate) fn of(entry: bool, shell: bool) -> Self {
        match (entry, shell) {
            (true, _) => Self::Entry,
            (false, true) => Self::Shell,
            (false, false) => Self::None,
        }
    }
}

/// Whether `rule` is an allow rule for the `browser` MCP server: `mcp__browser` or one of its
/// tools' `mcp__browser__<tool>`, by exact server name (`mcp__browsers` is another server's).
pub(super) fn is_browser_mcp_rule(rule: &str) -> bool {
    let server = crate::agent::mcp_trust::claude_rule(clauseo_profile::servers::BROWSER);
    rule.strip_prefix(server.as_str())
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("__"))
}
