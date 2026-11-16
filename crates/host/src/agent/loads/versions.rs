//! The `clauseo-versions` entry of one session: `clauseo-mcp connector clauseo-versions`, which
//! relays the agent's MCP lines to the session's version-history listener in the host
//! (`connectors/versions_relay.rs`), answered by the session's backend (`crate::versions`).
//!
//! The hook offers the entry with two tokens (`clauseo_profile::placeholders::IN_SERVERS`):
//! `{versions_port}` and `{versions_token_file}`. The host fills them here, once per session,
//! from the listener its launch prep opened (`actor/turn/launch_checks/versions.rs`), or drops
//! the entry. The entry goes last, and only under [`McpControl::Clauseo`]: under the firm's
//! `managed-mcp.json` no server goes. Its args and env name the token file, never the token. On
//! Windows it carries the same variables as the `case-law` entry (`case_law::windows_env`), which
//! the connector needs to reach loopback at all.
//!
//! Both tools only read, and only the session's matter, so they run without asking: Claude gets
//! one `mcp__clauseo-versions__<tool>` rule per tool ([`claude_allow`]), and the host answers
//! Codex's approval of either itself (`agent/mcp_trust.rs` `never_asks`); forced ask withholds
//! both, as it does case law's.

use std::path::Path;

use clauseo_acp::McpServer;
use clauseo_profile::placeholders::{self, IN_SERVERS, VERSIONS_PORT, VERSIONS_TOKEN_FILE};
use clauseo_profile::servers::VERSIONS;

use super::McpControl;

/// Our server as Claude names it: `mcp__clauseo-versions`, and its tools
/// `mcp__clauseo-versions__<tool>`. The name is reserved, so no other server of that name
/// reaches Claude.
pub(crate) const CLAUDE_SERVER: &str = "mcp__clauseo-versions";

/// The tools, each read-only and bound to the session's matter (`crate::versions`).
pub(crate) const TOOLS: [&str; 2] = ["list_versions", "read_version"];

/// Whether `tool` is one of [`TOOLS`], by exact name.
pub(crate) fn unasked_tool(tool: &str) -> bool {
    TOOLS.contains(&tool)
}

/// The pre-allow rules Claude gets when the entry is sent: one `mcp__clauseo-versions__<tool>`
/// per [`TOOLS`], never the whole server.
pub(crate) fn claude_allow() -> impl Iterator<Item = String> {
    TOOLS.iter().map(|tool| format!("{CLAUDE_SERVER}__{tool}"))
}

/// Whether `rule` is one of Claude's `allowedTools` for a `clauseo-versions` server: the server
/// itself or one of its tools. The host drops the hook's and adds its own only with its entry.
pub(crate) fn is_versions_rule(rule: &str) -> bool {
    rule.strip_prefix(CLAUDE_SERVER)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("__"))
}

/// What the `clauseo-versions` entry of one session needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VersionsInputs<'a> {
    /// The listener's port and token file, when the launch prep opened it.
    pub(crate) listener: Option<(u16, &'a Path)>,
    /// Who controls MCP on this machine, now.
    pub(crate) control: McpControl,
    /// On Windows, the variables the entry's env gets from the host (`case_law::windows_env`);
    /// none elsewhere.
    pub(crate) windows_env: &'a [(String, String)],
}

/// The hook's servers without any `clauseo-versions` entry, and the entry filled (none when the
/// session does not get it).
pub(crate) fn hook_servers(
    hook: Vec<McpServer>,
    inputs: &VersionsInputs<'_>,
) -> (Vec<McpServer>, Option<McpServer>) {
    let (offered, rest): (Vec<McpServer>, Vec<McpServer>) = hook
        .into_iter()
        .partition(|server| server.name() == VERSIONS);
    let entry = match (offered.into_iter().next(), inputs.listener) {
        (Some(entry), Some((port, token_file))) if inputs.control == McpControl::Clauseo => {
            fill(entry, port, token_file, inputs.windows_env)
        }
        _ => None,
    };
    (rest, entry)
}

/// The entry with its tokens filled, and each of `windows_env` added unless the entry already
/// names it (in any case); none when a value cannot go (a token file path that is not UTF-8),
/// the entry is not a stdio server or holds a token the host cannot fill.
fn fill(
    server: McpServer,
    port: u16,
    token_file: &Path,
    windows_env: &[(String, String)],
) -> Option<McpServer> {
    let Some(token_file) = dunce::simplified(token_file).to_str() else {
        tracing::warn!("the versions token file's path is not UTF-8: the tool is left out");
        return None;
    };
    let port = port.to_string();
    let values = [
        (VERSIONS_PORT, port.as_str()),
        (VERSIONS_TOKEN_FILE, token_file),
    ];
    let fill = |text: &str| placeholders::fill(text, &IN_SERVERS, &values);
    let McpServer::Stdio {
        name,
        command,
        args,
        env,
    } = server
    else {
        tracing::error!("the clauseo-versions entry is not a stdio server: the tool is left out");
        return None;
    };
    let filled = (|| -> Result<McpServer, placeholders::Unfilled> {
        let mut env: Vec<(String, String)> = env
            .iter()
            .map(|(key, value)| Ok((key.clone(), fill(value)?)))
            .collect::<Result<_, _>>()?;
        for (key, value) in windows_env {
            if !env.iter().any(|(own, _)| own.eq_ignore_ascii_case(key)) {
                env.push((key.clone(), value.clone()));
            }
        }
        Ok(McpServer::Stdio {
            name,
            command,
            args: args.iter().map(|arg| fill(arg)).collect::<Result<_, _>>()?,
            env,
        })
    })();
    filled
        .map_err(|unfilled| {
            tracing::error!(%unfilled, "the clauseo-versions entry has a token the host cannot fill");
        })
        .ok()
}

#[cfg(test)]
mod tests;
