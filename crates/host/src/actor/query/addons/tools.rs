//! `ListAddons`' tools: Clauseo's own `case-law` and `browser`, then the lawyer's MCP servers
//! as Settings › Tools shows them (`mcp/view.rs`), by name. None of them reads a file.
//!
//! - Under the firm's `managed-mcp.json` Settings › Tools shows no server; here the lawyer's
//!   own are listed `blocked`, off and locked, so they do not vanish.
//! - A remote server's address is shown as its scheme and host (and a port that is not the
//!   default) only, as Settings › Tools' list shows it: a path or a query may hold a token.
//!   Settings › Tools' `McpServerView.url` keeps it whole: its Edit form saves that field back.

use clauseo_protocol::{
    Addon, AddonFrom, AddonFromKind, AddonKind, AddonSource, AddonStatus, McpControl,
    McpServerKind, McpServerView,
};

use super::{Places, row};
use crate::actor::Actor;

/// The built-in tools, then the added ones, placed for `matter`.
pub(super) fn rows(actor: &Actor, matter: Option<clauseo_protocol::MatterId>) -> Vec<Addon> {
    let places = Places::of(actor, matter);
    let effective = &actor.effective;
    let tools = &effective.settings().tools;
    // Case law is left out until its credential has been read once (`recheck::status`).
    let case_law = crate::actor::connectors::recheck::status(actor).map(|status| Addon {
        description: "Searches Indian judgments and statutes on Indian Kanoon.".to_owned(),
        on: tools.connectors,
        status,
        locked: effective.is_locked("tools.connectors"),
        ..row(
            AddonKind::Tool,
            "case-law",
            "Case law (Indian Kanoon)",
            AddonSource::BuiltIn,
        )
    });
    let mut rows: Vec<Addon> = case_law.into_iter().collect();
    rows.extend([Addon {
        description: "Opens and reads web pages in a browser on this computer.".to_owned(),
        on: tools.browser,
        locked: effective.is_locked("tools.browser"),
        ..row(AddonKind::Tool, "browser", "Browser", AddonSource::BuiltIn)
    }]);
    rows = rows
        .into_iter()
        .map(|row| places.placed_as(row, true))
        .collect();
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
    let servers = effective.mcp_servers();
    rows.extend(view.servers.iter().map(|server| {
        let all = servers
            .get(&server.name)
            .is_none_or(|launch| !launch.matter_scoped());
        places.placed_as(added(server, lawyer), all)
    }));
    rows
}

/// `url`'s origin: scheme, host, and the port when it is not the scheme's default; `None` when
/// it does not parse (settings check every address they keep).
fn origin(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    url.host()
        .is_some()
        .then(|| url.origin().ascii_serialization())
}

/// A server's row. `lawyer`: the lawyer decides the list, so the row can be removed and is not
/// locked.
fn added(server: &McpServerView, lawyer: bool) -> Addon {
    let from = match server.kind {
        McpServerKind::Local => server.command.clone().map(|text| AddonFrom {
            kind: AddonFromKind::Program,
            text,
        }),
        McpServerKind::Remote => server
            .url
            .as_deref()
            .and_then(origin)
            .map(|text| AddonFrom {
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
        server: Some(server.clone()),
        ..row(
            AddonKind::Tool,
            &server.name,
            &server.name,
            AddonSource::Added,
        )
    }
}
