//! The `case-law` entry of one session (R24): `clauseo-mcp connector case-law`, which relays
//! the agent's MCP lines to the session's relay in the host (`connectors.rs`).
//!
//! The hook offers the entry with two tokens (`clauseo_profile::placeholders::IN_SERVERS`):
//! `{relay_port}` and `{relay_token_file}`. The host fills them here, once per session, from the
//! relay its launch prep opened (`actor/turn/launch_checks/relay.rs`), or drops the entry. The
//! entry goes after the lawyer's servers and the profile's other entries (`browser`), and only
//! under [`McpControl::Clauseo`]: under the firm's `managed-mcp.json` no server goes, so the
//! header says `ManagedByFirm`. Its args and env name the token file, never the token.
//!
//! On Windows the entry also carries `SystemRoot`, the host's own: Winsock cannot start without
//! it, and `clauseo-mcp connector` would then answer every call "relay not reachable". Both
//! agents' MCP clients pass their own `SystemRoot` on today; the entry does not rely on it.

use std::ffi::OsString;
use std::path::Path;

use clauseo_acp::McpServer;
use clauseo_profile::placeholders::{self, IN_SERVERS, RELAY_PORT, RELAY_TOKEN_FILE};
use clauseo_profile::servers::CASE_LAW;
use clauseo_protocol::{ClauseoTool, ToolAvailability, ToolState, ToolsUnavailableReason};

use super::McpControl;

/// The variable Windows networking needs (`SystemRoot`).
pub(crate) const SYSTEM_ROOT: &str = "SystemRoot";

/// The pre-allow rule Claude gets when the entry is sent (decision D7): every tool of our
/// `case-law` server. The name is reserved, so no other server of that name reaches Claude.
pub(crate) const CLAUDE_ALLOW: &str = "mcp__case-law";

/// What the launch prep made of the session's relay, as plain values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Relay<'a> {
    /// Nothing shows: no relay, and no header state.
    Absent,
    /// No relay, for this reason (the connector is added).
    Unavailable(ToolsUnavailableReason),
    /// The relay listens on `port`, and its token is in `token_file`.
    Open {
        /// The relay's loopback port.
        port: u16,
        /// `<data>/run/<session id>/relay-token`.
        token_file: &'a Path,
    },
}

/// What the `case-law` entry of one session needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CaseLawInputs<'a> {
    /// The session's relay.
    pub(crate) relay: Relay<'a>,
    /// Who controls MCP on this machine, now.
    pub(crate) control: McpControl,
    /// On Windows, the host's `SystemRoot` for the entry's env; `None` elsewhere.
    pub(crate) system_root: Option<&'a str>,
}

/// The session's case law: the hook's servers without any `case-law` entry, the entry filled
/// (none when the session does not get it), and its header state (none when nothing shows).
pub(crate) fn hook_servers(
    hook: Vec<McpServer>,
    inputs: &CaseLawInputs<'_>,
) -> (Vec<McpServer>, Option<McpServer>, Option<ToolState>) {
    let (offered, rest): (Vec<McpServer>, Vec<McpServer>) = hook.into_iter().partition(|server| {
        let McpServer::Stdio { name, .. } = server;
        name == CASE_LAW
    });
    let Some(entry) = offered.into_iter().next() else {
        return (rest, None, None);
    };
    let filled = match inputs.relay {
        Relay::Absent => return (rest, None, None),
        Relay::Unavailable(reason) => Err(reason),
        Relay::Open { .. } if inputs.control == McpControl::Managed => {
            Err(ToolsUnavailableReason::ManagedByFirm)
        }
        Relay::Open { port, token_file } => fill(entry, port, token_file, inputs.system_root),
    };
    let (entry, availability) = match filled {
        Ok(entry) => (Some(entry), ToolAvailability::Available),
        Err(reason) => (None, ToolAvailability::Unavailable { reason }),
    };
    let state = ToolState {
        tool: ClauseoTool::CaseLaw,
        availability,
    };
    (rest, entry, Some(state))
}

/// The entry with its tokens filled, and `SystemRoot` added when given; `NoRelay` when a value
/// cannot go (a token file path that is not UTF-8) or the entry holds a token the host cannot
/// fill.
fn fill(
    server: McpServer,
    port: u16,
    token_file: &Path,
    system_root: Option<&str>,
) -> Result<McpServer, ToolsUnavailableReason> {
    let Some(token_file) = dunce::simplified(token_file).to_str() else {
        tracing::warn!("the relay token file's path is not UTF-8: case law is left out");
        return Err(ToolsUnavailableReason::NoRelay);
    };
    let port = port.to_string();
    let values = [(RELAY_PORT, port.as_str()), (RELAY_TOKEN_FILE, token_file)];
    let fill = |text: &str| placeholders::fill(text, &IN_SERVERS, &values);
    let McpServer::Stdio {
        name,
        command,
        args,
        env,
    } = server;
    let filled = (|| -> Result<McpServer, placeholders::Unfilled> {
        let mut env: Vec<(String, String)> = env
            .iter()
            .map(|(key, value)| Ok((key.clone(), fill(value)?)))
            .collect::<Result<_, _>>()?;
        if let Some(root) = system_root
            && !env
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(SYSTEM_ROOT))
        {
            env.push((SYSTEM_ROOT.to_owned(), root.to_owned()));
        }
        Ok(McpServer::Stdio {
            name,
            command,
            args: args.iter().map(|arg| fill(arg)).collect::<Result<_, _>>()?,
            env,
        })
    })();
    filled.map_err(|unfilled| {
        tracing::error!(%unfilled, "the case-law entry has a token the host cannot fill");
        ToolsUnavailableReason::NoRelay
    })
}

/// The `SystemRoot` the entry carries: on Windows (`windows`), the one in `inherited` (the
/// environment the agent's spec starts from), its name matched in any case; else none.
pub(crate) fn system_root(
    windows: bool,
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) -> Option<String> {
    if !windows {
        return None;
    }
    let root = inherited
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(SYSTEM_ROOT))
        .and_then(|(_, value)| value.into_string().ok());
    if root.is_none() {
        tracing::warn!("the host has no SystemRoot to give the case-law entry");
    }
    root
}

/// Claude's `allowedTools` without any rule for a `case-law` server: `mcp__case-law` itself
/// and its `mcp__case-law__<tool>` rules. The host adds its own back only with its entry.
pub(crate) fn is_case_law_rule(rule: &str) -> bool {
    rule.strip_prefix(CLAUDE_ALLOW)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("__"))
}

#[cfg(test)]
mod tests;
