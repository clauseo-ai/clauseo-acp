//! `ListAddons`' tools: Clauseo's own `case-law` and `browser`, then the lawyer's MCP servers
//! as Settings › Tools shows them (`mcp/view.rs`), by name. None of them reads a file.

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
    let lawyer = view.control == McpControl::Lawyer;
    rows.extend(view.servers.iter().map(|server| added(server, lawyer)));
    rows
}

/// A server's row. `lawyer`: the lawyer decides the list, so the row can be removed and is not
/// locked.
fn added(server: &McpServerView, lawyer: bool) -> Addon {
    let from = match server.kind {
        McpServerKind::Local => server.command.clone().map(|text| AddonFrom {
            kind: AddonFromKind::Program,
            text,
        }),
        McpServerKind::Remote => server.url.clone().map(|text| AddonFrom {
            kind: AddonFromKind::Address,
            text,
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
