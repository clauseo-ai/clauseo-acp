//! `ListAddons`' tools: the lawyer's MCP servers, local and remote, a firm's list, and
//! `AddonsChanged` after a save; case law's `needs_setup` until it is connected. Split from
//! `addons.rs` for its length.

use std::sync::Arc;
use std::time::Duration;

use clauseo_connectors::MemoryStore;
use clauseo_connectors::fake::Fixture;
use clauseo_protocol::{
    AddonFrom, AddonFromKind, AddonKind, AddonSource, AddonStatus, ConnectorCommand,
};
use serde_json::json;

use crate::addons::{addons, addons_changed, item};
use crate::connector_rig::Rig as ConnectorRig;
use crate::connector_rig::{Rig, Start, add_key, signed_in};
use crate::mcp_tools::{local, save, store};
use crate::support::{Dir, wait};

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
            text: "https://mcp.linear.app/mcp".to_owned()
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

/// Case law is not `ready` until a key is added; connecting sends `AddonsChanged`, after
/// which it is `ready`.
#[test]
fn connecting_case_law_sends_addons_changed_and_ends_needs_setup() {
    let dir = Dir::new("addons_case_law", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    let before = item(&addons(&rig.host), AddonKind::Tool, "case-law").clone();
    assert_ne!(before.status, AddonStatus::Ready, "{before:?}");
    assert!(before.on, "{before:?}");
    rig.ok(ConnectorCommand::Check {});
    let key = rig.fake.new_api_key();
    rig.keys.push(key.clone());
    rig.ok(add_key(&key));
    rig.state_until("signed in", |state| signed_in(state).is_some());
    addons_changed(&mut rig.sub);
    let after = item(&addons(&rig.host), AddonKind::Tool, "case-law").clone();
    assert_eq!(after.status, AddonStatus::Ready, "{after:?}");
    rig.assert_no_secret_seen();
}

/// The `pricing.json` fetches the fake has had.
fn pricing_fetches(rig: &ConnectorRig) -> usize {
    rig.fake.requests_to("/pricing.json").len()
}

/// Waits until the fake has had `count` `pricing.json` fetches.
fn fetched(rig: &ConnectorRig, count: usize) {
    wait("the pricing.json fetch", async {
        while pricing_fetches(rig) < count {
            async_io::Timer::after(Duration::from_millis(5)).await;
        }
    });
}

/// A read of the list while the check fails lists case law `unreachable` and starts one check
/// in the background; reads within 30 s start none; a read after 30 s starts one that finds it
/// live, which sends `AddonsChanged`, and the list then says `needs_setup`.
#[test]
fn an_unreachable_case_law_is_checked_again_when_the_list_is_read() {
    let dir = Dir::new("addons_case_law_recheck", &[]);
    let mut rig = Rig::start(&dir, Start::default());
    rig.fake.set_pricing(Fixture::Raw {
        status: 500,
        content_type: "text/plain".to_owned(),
        body: b"down".to_vec(),
    });
    let clock = rig.host.clock.clone();
    let first = item(&addons(&rig.host), AddonKind::Tool, "case-law").clone();
    assert_eq!(first.status, AddonStatus::Unreachable, "{first:?}");
    fetched(&rig, 1);

    // Within 30 s of that check's start: no second check, however often the list is read.
    rig.fake.set_pricing(Fixture::File("pricing-live.json"));
    clock.advance(Duration::from_secs(29));
    for _ in 0..3 {
        let again = item(&addons(&rig.host), AddonKind::Tool, "case-law").clone();
        assert_eq!(again.status, AddonStatus::Unreachable, "{again:?}");
    }
    rig.host.settle();
    assert_eq!(pricing_fetches(&rig), 1);

    // Past 30 s: the next read starts one (once the first has ended), which finds it live.
    clock.advance(Duration::from_secs(2));
    // Each read is a query the test waits on, so the pause between reads is a timer of its own.
    for _ in 0..4_000 {
        if pricing_fetches(&rig) >= 2 {
            break;
        }
        let _ = addons(&rig.host);
        wait("a pause", async_io::Timer::after(Duration::from_millis(5)));
    }
    assert_eq!(pricing_fetches(&rig), 2, "no second check");
    addons_changed(&mut rig.sub);
    let after = item(&addons(&rig.host), AddonKind::Tool, "case-law").clone();
    assert_eq!(after.status, AddonStatus::NeedsSetup, "{after:?}");
    assert_eq!(pricing_fetches(&rig), 2);
}
