//! `ListAddons`' tools: the lawyer's MCP servers, local and remote, a firm's list, the firm's
//! `managed-mcp.json`, an address's query left out, and `AddonsChanged` after a save.

use std::sync::Arc;

use clauseo_connectors::MemoryStore;
use clauseo_protocol::{Addon, AddonFrom, AddonFromKind, AddonKind, AddonSource, AddonStatus};
use serde_json::json;

use super::{addons, addons_changed, drain, item};
use crate::mcp_tools::{local, save, store};
use crate::support::Dir;

/// A local and a remote server: each row has its program or address, its switch, no
/// description, and the lawyer may remove it. A save sends `AddonsChanged` and adds a row.
#[test]
fn mcp_servers_are_listed_and_a_save_sends_addons_changed() {
    let dir = Dir::new("addons_mcp", &[]);
    dir.set_settings(
        &[],
        json!({"mcp": {"servers": {
            "linear": {"url": "https://mcp.linear.app/mcp", "enabled": true},
            "github": {"command": "npx", "args": ["-y", "server-github"], "enabled": false},
        }}}),
    );
    let store = store(&[]);
    let host = dir.start_with(0, |config| {
        config.with_mcp_secrets(Arc::<MemoryStore>::clone(&store))
    });
    let (mut sub, _first) = crate::settings::subscribe_app(&host);
    drain(&host, &mut sub);
    let items = addons(&host);
    let tools: Vec<&str> = items
        .iter()
        .filter(|item| item.kind == AddonKind::Tool)
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(tools, ["case-law", "browser", "github", "linear"]);
    let github = item(&items, AddonKind::Tool, "github");
    assert_eq!(
        github.from,
        Some(AddonFrom {
            kind: AddonFromKind::Program,
            text: "npx".to_owned()
        })
    );
    assert_eq!((github.on, github.status), (false, AddonStatus::Ready));
    assert_eq!(
        (github.name.as_str(), github.description.as_str()),
        ("github", "")
    );
    assert_eq!(
        (github.source, github.has_scripts),
        (AddonSource::Added, None)
    );
    assert!(github.can_remove && !github.locked, "{github:?}");
    let linear = item(&items, AddonKind::Tool, "linear");
    assert_eq!(
        linear.from,
        Some(AddonFrom {
            kind: AddonFromKind::Address,
            text: "https://mcp.linear.app".to_owned()
        })
    );
    assert!(linear.on, "{linear:?}");

    host.ok(save(None, local("notes", "npx", Vec::new())));
    addons_changed(&mut sub);
    let notes = item(&addons(&host), AddonKind::Tool, "notes").clone();
    assert!(notes.on && notes.can_remove, "{notes:?}");
}

/// Under a firm's `mcp.servers` its servers are locked and cannot be removed.
#[test]
fn a_firms_servers_are_locked() {
    let dir = Dir::new("addons_mcp_firm", &[]);
    dir.set_policy(&json!({"mcp": {"servers": {"firm": {"command": "firm-tool"}}}}).to_string());
    let host = dir.start(0);
    let items = addons(&host);
    let firm = item(&items, AddonKind::Tool, "firm");
    assert!(firm.locked && !firm.can_remove, "{firm:?}");
}

/// Under the firm's `managed-mcp.json` the lawyer's own servers stay listed: `blocked`, off,
/// locked, and not removable.
#[test]
fn under_a_managed_mcp_file_the_lawyers_servers_are_blocked() {
    let dir = Dir::new("addons_mcp_managed", &[]);
    dir.set_settings(
        &[],
        json!({"mcp": {"servers": {
            "github": {"command": "npx", "args": ["-y", "server-github"], "enabled": true},
        }}}),
    );
    dir.set_managed_mcp();
    let host = dir.start(0);
    let items = addons(&host);
    let github = item(&items, AddonKind::Tool, "github");
    assert_eq!(
        (github.status, github.on),
        (AddonStatus::Blocked, false),
        "{github:?}"
    );
    assert!(github.locked && !github.can_remove, "{github:?}");
}

/// A remote server's address is listed as its scheme and host only: a token in its path (as
/// Zapier's addresses carry one) or its query never reaches the list;
/// `server` keeps the whole URL, as Settings › Tools shows it, since Edit saves it back.
#[test]
fn an_address_is_listed_as_its_host_only() {
    let dir = Dir::new("addons_mcp_query", &[]);
    dir.set_settings(
        &[],
        json!({"mcp": {"servers": {
            "notes": {"url": "https://mcp.example.com/mcp/s/abc123/mcp?token=def456", "enabled": true},
            "local": {"url": "http://127.0.0.1:8123/mcp", "enabled": true},
        }}}),
    );
    let host = dir.start(0);
    let items = addons(&host);
    let notes = item(&items, AddonKind::Tool, "notes");
    assert_eq!(
        notes.from.as_ref().map(|from| from.text.as_str()),
        Some("https://mcp.example.com")
    );
    let shown = Addon {
        server: None,
        ..notes.clone()
    };
    let text = format!("{shown:?}");
    assert!(
        !text.contains("abc123") && !text.contains("def456"),
        "{text}"
    );
    let local = item(&items, AddonKind::Tool, "local");
    assert_eq!(
        local.from.as_ref().map(|from| from.text.as_str()),
        Some("http://127.0.0.1:8123")
    );
    let url = notes
        .server
        .as_ref()
        .and_then(|server| server.url.as_deref());
    assert_eq!(
        url,
        Some("https://mcp.example.com/mcp/s/abc123/mcp?token=def456")
    );
}
