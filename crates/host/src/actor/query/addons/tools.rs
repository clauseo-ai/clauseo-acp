//! `ListAddons`' tools: Clauseo's own `case-law` and `browser`, then the lawyer's MCP servers
//! as Settings › Tools shows them (`mcp/view.rs`), by name. None of them reads a file.
//!
//! - Under the firm's `managed-mcp.json` Settings › Tools shows no server; here the lawyer's
//!   own are listed `blocked`, off and locked, so they do not vanish.
//! - A remote server's address is shown without its query or fragment, which may hold a token.
//!   Settings › Tools' `McpServerView.url` keeps it whole: its Edit form saves that field back.

use clauseo_protocol::{
    Addon, AddonFrom, AddonFromKind, AddonKind, AddonSource, AddonStatus, McpControl,
    McpServerKind, McpServerView,
};

use super::row;
use crate::actor::Actor;

/// The built-in tools, then the added ones.
pub(super) fn rows(actor: &Actor) -> Vec<Addon> {
    let effective = &actor.effective;
    let tools = &effective.settings().tools;
    let mut rows = vec![
        Addon {
            description: "Searches Indian judgments and statutes on Indian Kanoon.".to_owned(),
            on: tools.connectors,
            status: crate::actor::connectors::recheck::status(actor),
            locked: effective.is_locked("tools.connectors"),
            ..row(
                AddonKind::Tool,
                "case-law",
                "Case law (Indian Kanoon)",
                AddonSource::BuiltIn,
            )
        },
        Addon {
            description: "Opens and reads web pages in a browser on this computer.".to_owned(),
            on: tools.browser,
            locked: effective.is_locked("tools.browser"),
            ..row(AddonKind::Tool, "browser", "Browser", AddonSource::BuiltIn)
        },
    ];
    let view = crate::actor::mcp::view::view(actor);
    if view.control == McpControl::FirmManaged {
        let servers = effective.mcp_servers().iter();
        rows.extend(servers.map(|(name, server)| {
            let server = crate::actor::mcp::view::server_view(actor, name, server);
            Addon {
                on: false,
                status: AddonStatus::Blocked,
                ..added(&server, false)
            }
        }));
        return rows;
    }
    let lawyer = view.control == McpControl::Lawyer;
    rows.extend(view.servers.iter().map(|server| added(server, lawyer)));
    rows
}

/// `url` without its query or fragment.
fn without_query(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).to_owned()
}

/// A server's row. `lawyer`: the lawyer decides the list, so the row can be removed and is not
/// locked.
fn added(server: &McpServerView, lawyer: bool) -> Addon {
    let from = match server.kind {
        McpServerKind::Local => server.command.clone().map(|text| AddonFrom {
            kind: AddonFromKind::Program,
            text,
        }),
        McpServerKind::Remote => server.url.as_deref().map(|url| AddonFrom {
            kind: AddonFromKind::Address,
            text: without_query(url),
        }),
    };
    let unsaved = server
        .env
        .iter()
        .chain(&server.headers)
        .any(|secret| !secret.saved);
    Addon {
        from,
        on: server.enabled,
        status: if unsaved {
            AddonStatus::NeedsSecret
        } else {
            AddonStatus::Ready
        },
        can_remove: lawyer,
        locked: !lawyer,
        ..row(
            AddonKind::Tool,
            &server.name,
            &server.name,
            AddonSource::Added,
        )
    }
}
