//! The profile hook's MCP servers for one session, and above all its `browser` entry (S27's
//! agent-browser behind `clauseo-mcp browser-guard`, decision record 0030).
//!
//! The hook sees only the agent's id, so the entry comes with tokens
//! (`clauseo_profile::placeholders::IN_SERVERS`). The host fills them here, once per session,
//! or drops the entry and says why in the thread header. In this order:
//!
//! 1. `tools.browser` off: `Off`, or `LockedOff` when policy set it (or could not be read).
//! 2. The firm's `managed-mcp.json` ([`McpControl::Managed`]): no Clauseo server at all goes,
//!    so `ManagedByFirm`.
//! 3. No free browser slot: `TooManyBrowsers`.
//! 4. No proxy port for the session: `NoProxy` (every session until stage C's proxy).
//! 5. No Chrome picked, a sidecar (`clauseo-mcp` or agent-browser) missing, or the session's
//!    ports or socket folder not claimed (`browser::session`): `BrowserNotInstalled`.
//!
//! Each value is filled into one decoded string (one argument, one variable's value) in one
//! pass, so a folder whose name holds a token stays literal; MCP arguments go as a list with no
//! shell, so nothing more is escaped.

use std::path::Path;

use clauseo_acp::McpServer;
use clauseo_profile::placeholders::{
    self, BROWSER_DIR, BROWSER_EXECUTABLE, CDP_PORT, DAEMON_PORT, IN_SERVERS, PROXY_PORT,
    SESSION_ID, SOCKET_DIR, STREAM_PORT,
};
use clauseo_profile::servers::BROWSER;
use clauseo_protocol::{ClauseoTool, ToolAvailability, ToolState, ToolsUnavailableReason};

use super::McpControl;

/// A `tools.*` switch as the settings hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Switch {
    /// Its value.
    pub(crate) on: bool,
    /// Whether policy set it, or an unreadable policy source locked it.
    pub(crate) locked: bool,
}

impl Switch {
    /// The reason an off switch gives: `Off` for the lawyer's, `LockedOff` for the firm's.
    pub(crate) fn off_reason(self) -> ToolsUnavailableReason {
        if self.locked {
            ToolsUnavailableReason::LockedOff
        } else {
            ToolsUnavailableReason::Off
        }
    }
}

/// What the `browser` entry of one session needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BrowserInputs<'a> {
    /// `tools.browser`.
    pub(crate) switch: Switch,
    /// Who controls MCP on this machine.
    pub(crate) control: McpControl,
    /// Whether this session holds a browser slot or one is free (`browser::limits`).
    pub(crate) slot_free: bool,
    /// The proxy's port for this session, if there is a proxy.
    pub(crate) port: Option<u16>,
    /// The Chrome picked for this session, if any.
    pub(crate) executable: Option<&'a Path>,
    /// The session's browser state folder, `<data>/run/<session id>/browser`.
    pub(crate) browser_dir: &'a Path,
    /// The session's claimed ports, socket folder and agent-browser name, if they were claimed.
    pub(crate) claims: Option<Claims<'a>>,
}

/// What a session's `browser::session::BrowserSession` fills: plain values, so the fill is
/// tested without claiming anything.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Claims<'a> {
    /// agent-browser's session name, `{session_id}`.
    pub(crate) name: &'a str,
    /// `{cdp_port}`.
    pub(crate) cdp_port: u16,
    /// `{stream_port}`.
    pub(crate) stream_port: u16,
    /// `{daemon_port}`, Windows only.
    pub(crate) daemon_port: Option<u16>,
    /// `{socket_dir}`, macOS and Linux only: the folder as made (`/tmp/...`), never its
    /// canonical form.
    pub(crate) socket_dir: Option<&'a Path>,
}

impl<'a> Claims<'a> {
    /// The values of `session`.
    pub(crate) fn of(session: &'a crate::browser::session::BrowserSession) -> Self {
        Self {
            name: &session.name,
            cdp_port: session.ports.cdp.port(),
            stream_port: session.ports.stream.port(),
            daemon_port: session
                .ports
                .daemon
                .as_ref()
                .map(clauseo_sandbox::ports::PortClaim::port),
            #[cfg(unix)]
            socket_dir: Some(session.socket_dir.path()),
            #[cfg(not(unix))]
            socket_dir: None,
        }
    }
}

/// The hook's servers for this session, tokens filled, and the browser's header state (none
/// when the hook offers no `browser`). Under [`McpControl::Managed`] no server goes at all.
pub(crate) fn hook_servers(
    hook: Vec<McpServer>,
    inputs: &BrowserInputs<'_>,
) -> (Vec<McpServer>, Option<ToolState>) {
    let mut servers = Vec::new();
    let mut state = None;
    for server in hook {
        let McpServer::Stdio { name, .. } = &server;
        if name != BROWSER {
            if inputs.control == McpControl::Clauseo {
                servers.push(server);
            }
            continue;
        }
        let availability = match browser_entry(server, inputs) {
            Ok(filled) => {
                servers.push(filled);
                ToolAvailability::Available
            }
            Err(reason) => ToolAvailability::Unavailable { reason },
        };
        state = Some(ToolState {
            tool: ClauseoTool::Browser,
            availability,
        });
    }
    (servers, state)
}

/// The `browser` entry with its tokens filled, or why it is left out.
fn browser_entry(
    server: McpServer,
    inputs: &BrowserInputs<'_>,
) -> Result<McpServer, ToolsUnavailableReason> {
    if !inputs.switch.on {
        return Err(inputs.switch.off_reason());
    }
    if inputs.control == McpControl::Managed {
        return Err(ToolsUnavailableReason::ManagedByFirm);
    }
    if !inputs.slot_free {
        return Err(ToolsUnavailableReason::TooManyBrowsers);
    }
    let port = inputs
        .port
        .ok_or(ToolsUnavailableReason::NoProxy)?
        .to_string();
    let executable = inputs
        .executable
        .ok_or(ToolsUnavailableReason::BrowserNotInstalled)?;
    let claims = inputs
        .claims
        .ok_or(ToolsUnavailableReason::BrowserNotInstalled)?;
    let (Some(executable), Some(browser_dir)) = (
        executable.to_str(),
        dunce::simplified(inputs.browser_dir).to_str(),
    ) else {
        tracing::warn!(
            chrome = %executable.display(),
            folder = %inputs.browser_dir.display(),
            "the Chrome or the session's browser folder is not UTF-8: the browser is left out"
        );
        return Err(ToolsUnavailableReason::BrowserNotInstalled);
    };
    let cdp_port = claims.cdp_port.to_string();
    let stream_port = claims.stream_port.to_string();
    let daemon_port = claims.daemon_port.map(|port| port.to_string());
    let socket_dir = match claims.socket_dir.map(Path::to_str) {
        Some(Some(text)) => Some(text),
        Some(None) => {
            tracing::warn!("the browser's socket folder is not UTF-8: the browser is left out");
            return Err(ToolsUnavailableReason::BrowserNotInstalled);
        }
        None => None,
    };
    // A token whose value is absent here (`{daemon_port}` off Windows, `{socket_dir}` on
    // Windows) is unfillable if the entry holds it, which leaves the browser out below.
    let mut values = vec![
        (BROWSER_DIR, browser_dir),
        (PROXY_PORT, port.as_str()),
        (SESSION_ID, claims.name),
        (BROWSER_EXECUTABLE, executable),
        (CDP_PORT, cdp_port.as_str()),
        (STREAM_PORT, stream_port.as_str()),
    ];
    values.extend(daemon_port.as_deref().map(|port| (DAEMON_PORT, port)));
    values.extend(socket_dir.map(|dir| (SOCKET_DIR, dir)));
    let fill = |text: &str| placeholders::fill(text, &IN_SERVERS, &values);
    let McpServer::Stdio {
        name,
        command,
        args,
        env,
    } = server;
    let filled = (|| -> Result<McpServer, placeholders::Unfilled> {
        Ok(McpServer::Stdio {
            name,
            command,
            args: args.iter().map(|arg| fill(arg)).collect::<Result<_, _>>()?,
            env: env
                .iter()
                .map(|(key, value)| Ok((key.clone(), fill(value)?)))
                .collect::<Result<_, _>>()?,
        })
    })();
    filled.map_err(|unfilled| {
        // A token the profile sends that this OS's claims do not fill: leave the browser out.
        tracing::error!(%unfilled, "the browser entry has a token the host cannot fill");
        ToolsUnavailableReason::BrowserNotInstalled
    })
}
