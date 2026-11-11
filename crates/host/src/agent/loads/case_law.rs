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
//! On Windows the entry also carries the variables the connector's process needs and the host
//! has (`windows_env`): `SystemRoot` and `windir` (Winsock cannot start without them, and
//! `clauseo-mcp connector` would then answer every call "relay not reachable"), and `TEMP`, `TMP`
//! and `PATHEXT` when set. Both agents' MCP clients pass their own `SystemRoot` on today; the
//! entry does not rely on it. Nothing else is added: no path, no permission.

use std::ffi::OsString;
use std::path::Path;

use clauseo_acp::McpServer;
use clauseo_profile::placeholders::{self, IN_SERVERS, RELAY_PORT, RELAY_TOKEN_FILE};
use clauseo_profile::servers::CASE_LAW;
use clauseo_protocol::{ClauseoTool, ToolAvailability, ToolState, ToolsUnavailableReason};

use super::McpControl;

/// The variable Windows networking needs (`SystemRoot`).
pub(crate) const SYSTEM_ROOT: &str = "SystemRoot";
/// The Windows directory's other name; Winsock and the runtime read it as well.
const WINDIR: &str = "windir";
/// Passed to the entry on Windows only when the host has them.
const OPTIONAL_WINDOWS_VARS: [&str; 3] = ["TEMP", "TMP", "PATHEXT"];

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
    /// On Windows, the variables the entry's env gets from the host (`windows_env`); none
    /// elsewhere.
    pub(crate) windows_env: &'a [(String, String)],
}

/// The session's case law: the hook's servers without any `case-law` entry, the entry filled
/// (none when the session does not get it), and its header state (none when nothing shows).
pub(crate) fn hook_servers(
    hook: Vec<McpServer>,
    inputs: &CaseLawInputs<'_>,
) -> (Vec<McpServer>, Option<McpServer>, Option<ToolState>) {
    let (offered, rest): (Vec<McpServer>, Vec<McpServer>) = hook
        .into_iter()
        .partition(|server| server.name() == CASE_LAW);
    let Some(entry) = offered.into_iter().next() else {
        return (rest, None, None);
    };
    let filled = match inputs.relay {
        Relay::Absent => return (rest, None, None),
        Relay::Unavailable(reason) => Err(reason),
        Relay::Open { .. } if inputs.control == McpControl::Managed => {
            Err(ToolsUnavailableReason::ManagedByFirm)
        }
        Relay::Open { port, token_file } => fill(entry, port, token_file, inputs.windows_env),
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

/// The entry with its tokens filled, and each of `windows_env` added unless the entry already
/// names it (in any case); `NoRelay` when a value cannot go (a token file path that is not UTF-8)
/// or the entry holds a token the host cannot fill.
fn fill(
    server: McpServer,
    port: u16,
    token_file: &Path,
    windows_env: &[(String, String)],
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
    } = server
    else {
        tracing::error!("the case-law entry is not a stdio server: case law is left out");
        return Err(ToolsUnavailableReason::NoRelay);
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
    filled.map_err(|unfilled| {
        tracing::error!(%unfilled, "the case-law entry has a token the host cannot fill");
        ToolsUnavailableReason::NoRelay
    })
}

/// The variables the entry's env carries on Windows (`windows`), in this order: `SystemRoot` and
/// `windir`, each read through `lookup` (the host's environment, the name matched in any case),
/// and when that has none, from `windows_dir` (the Windows directory); then `TEMP`, `TMP` and
/// `PATHEXT`, each only when `lookup` has it. A value that is not UTF-8 counts as absent. Empty
/// when `windows` is false.
pub(crate) fn windows_env(
    windows: bool,
    lookup: impl Fn(&str) -> Option<OsString>,
    windows_dir: impl Fn() -> Option<OsString>,
) -> Vec<(String, String)> {
    if !windows {
        return Vec::new();
    }
    let text = |value: OsString| value.into_string().ok();
    let mut env = Vec::new();
    for name in [SYSTEM_ROOT, WINDIR] {
        let value = lookup(name)
            .and_then(text)
            .or_else(|| windows_dir().and_then(text));
        if let Some(value) = value {
            env.push((name.to_owned(), value));
        } else {
            tracing::warn!(
                name,
                "the host has no Windows directory for the case-law entry"
            );
        }
    }
    for name in OPTIONAL_WINDOWS_VARS {
        if let Some(value) = lookup(name).and_then(text) {
            env.push((name.to_owned(), value));
        }
    }
    env
}

/// The Windows directory (`GetWindowsDirectoryW`), the fallback for `SystemRoot` and `windir`.
#[cfg(windows)]
#[expect(
    unsafe_code,
    reason = "GetWindowsDirectoryW, the Win32 call the windows crate binds; its buffer lives across it"
)]
pub(crate) fn windows_directory() -> Option<OsString> {
    use std::os::windows::ffi::OsStringExt;

    use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;

    /// The longest directory read; `MAX_PATH` is 260.
    const MAX_DIR: usize = 1024;
    let mut buffer = vec![0u16; MAX_DIR];
    // SAFETY: the call writes at most `buffer.len()` units into the live `buffer`.
    let len = unsafe { GetWindowsDirectoryW(Some(&mut buffer)) };
    let len = usize::try_from(len)
        .ok()
        .filter(|&len| len > 0 && len < buffer.len())?;
    buffer.truncate(len);
    Some(OsString::from_wide(&buffer))
}

/// Not Windows: there is no Windows directory to read.
#[cfg(not(windows))]
pub(crate) fn windows_directory() -> Option<OsString> {
    None
}

/// Claude's `allowedTools` without any rule for a `case-law` server: `mcp__case-law` itself
/// and its `mcp__case-law__<tool>` rules. The host adds its own back only with its entry.
pub(crate) fn is_case_law_rule(rule: &str) -> bool {
    rule.strip_prefix(CLAUDE_ALLOW)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with("__"))
}

#[cfg(test)]
mod tests;
