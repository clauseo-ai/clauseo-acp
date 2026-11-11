//! What Settings › Tools shows ([`McpToolsView`]): who decides the list, each server with
//! its variables' and headers' names, never a value, what other apps set up that would import,
//! and the last import's result. It reaches every app subscriber as
//! `McpToolsChanged` whenever it changes (a command, or a launch that found a keychain value
//! missing) and in every app catch-up.

use clauseo_protocol::{
    HostEvent, McpControl, McpSecretName, McpServerKind, McpServerView, McpToolsView,
};
use clauseo_settings::{McpServerLaunch, McpTransport, McpValue};

use super::super::Actor;

/// The view now. Under the firm's `managed-mcp.json` the list is not shown; while policy sets
/// `mcp.servers` it is the firm's, read-only.
pub(crate) fn view(actor: &Actor) -> McpToolsView {
    if managed(actor) {
        return McpToolsView {
            control: McpControl::FirmManaged,
            servers: Vec::new(),
            importable: Vec::new(),
            last_import: actor.mcp.last_import.clone(),
        };
    }
    let control = if actor.effective.mcp_locked() {
        McpControl::FirmList
    } else {
        McpControl::Lawyer
    };
    let servers = actor
        .effective
        .mcp_servers()
        .iter()
        .map(|(name, server)| server_view(actor, name, server))
        .collect();
    let importable = match control {
        McpControl::Lawyer => super::import::importable(actor),
        McpControl::FirmList | McpControl::FirmManaged => Vec::new(),
    };
    McpToolsView {
        control,
        servers,
        importable,
        last_import: actor.mcp.last_import.clone(),
    }
}

/// Whether the firm's `managed-mcp.json` is on this computer: one `stat`, as each launch makes
/// (`agent::loads::mcp_control`), without its log line.
pub(crate) fn managed(actor: &Actor) -> bool {
    std::fs::metadata(&actor.managed_mcp).is_ok()
}

fn server_view(actor: &Actor, name: &str, server: &McpServerLaunch) -> McpServerView {
    let names = |values: &std::collections::BTreeMap<String, McpValue>, env: bool| {
        values
            .iter()
            .map(|(var, value)| McpSecretName {
                name: var.clone(),
                saved: match value {
                    McpValue::Plain(_) => true,
                    // Only a local server's variables are read at a launch so far.
                    McpValue::Keychain(_) => {
                        !(env && actor.mcp.missing.contains(&(name.to_owned(), var.clone())))
                    }
                },
            })
            .collect()
    };
    match &server.transport {
        McpTransport::Local { command, args, env } => McpServerView {
            name: name.to_owned(),
            kind: McpServerKind::Local,
            command: Some(command.clone()),
            args: args.clone(),
            url: None,
            env: names(env, true),
            headers: Vec::new(),
            enabled: server.enabled,
        },
        McpTransport::Remote { url, headers } => McpServerView {
            name: name.to_owned(),
            kind: McpServerKind::Remote,
            command: None,
            args: Vec::new(),
            url: Some(url.clone()),
            env: Vec::new(),
            headers: names(headers, false),
            enabled: server.enabled,
        },
    }
}

/// Sends `McpToolsChanged` to every app subscriber when the view differs from the last one sent.
pub(crate) fn publish(actor: &mut Actor) {
    let view = view(actor);
    if actor.mcp.sent.as_ref() == Some(&view) {
        return;
    }
    actor.mcp.sent = Some(view.clone());
    super::super::app::publish(actor, &HostEvent::McpToolsChanged(view));
}

/// The app catch-up's part: the whole view, always (a lagged window may hold a list since
/// emptied). It leaves the one sent as it was, so a change the other subscribers have not heard
/// still goes to them.
pub(crate) fn current(actor: &Actor) -> HostEvent {
    HostEvent::McpToolsChanged(view(actor))
}
