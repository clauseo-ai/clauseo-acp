//! A matter's switch stored before a firm's lock (decision record 0040): once policy locks the
//! item's setting with scope `all`, the stored "off in X" counts for nothing, in the list and at
//! launch, since the lawyer can no longer clear it.

use clauseo_protocol::AddonKind;
use serde_json::json;

use super::scope::{row, set_on, two_matters};
use super::scope_launch::{server_env, with_servers};
use crate::mcp_tools::{finds_npx, npx, store, wired};
use crate::packs::put_pack;
use crate::profile_rig::turn;
use crate::support::{Dir, FAKE};

/// A pack, a server and the browser switched off in X, then locked by the firm with scope
/// `all`: X's list shows each on there with no switch.
#[test]
fn a_switch_stored_before_the_firms_lock_is_not_listed() {
    let dir = Dir::new("addons_lock_stored_list", &[]);
    let hash = put_pack(&dir, "lit-kit", &["lit-review"], b"Notes.\n");
    dir.set_settings(
        &[],
        json!({"skills": {"extra": "any", "enabled": {"lit-kit": hash}},
            "tools": {"browser": true},
            "mcp": {"servers": {"linear": {"command": "npx", "enabled": true}}}}),
    );
    let host = dir.start(0);
    let (x, _) = two_matters(&dir, &host);
    let items = [
        (AddonKind::Skill, "lit-kit"),
        (AddonKind::Tool, "linear"),
        (AddonKind::Tool, "browser"),
    ];
    for (kind, id) in items {
        host.ok(set_on(kind, id, x, false));
        let off = row(&host, Some(x), kind, id);
        assert!(!off.on && off.switched == [x], "{off:?}");
    }
    host.shutdown();
    let policy = json!({"skills": {"extra": "any", "enabled": {"lit-kit": hash}},
        "tools": {"browser": true},
        "mcp": {"servers": {"linear": {"command": "npx", "enabled": true}}}});
    let host = dir.start_with_policy(1, &policy.to_string());
    for (kind, id) in items {
        let locked = row(&host, Some(x), kind, id);
        assert!(locked.on && locked.switched.is_empty(), "{locked:?}");
    }
    host.shutdown();
}

/// A server switched off in X, then put in the firm's `mcp.servers` with scope `all`: X's
/// `session/new` carries it (the scenario checks `mcpServers` exactly).
#[test]
fn a_switch_stored_before_the_firms_lock_leaves_the_launch() {
    let dir = Dir::new("addons_lock_stored_launch", &[]);
    let npx = npx(&dir);
    let server = json!([{"name": "github", "command": npx.display().to_string(),
        "args": ["-y", "server-github"], "env": server_env(&npx)}]);
    let in_x = with_servers(&dir, "lock-server-x", &server);
    let entry = json!({"command": "npx", "args": ["-y", "server-github"], "enabled": true});
    dir.set_settings(
        &[(FAKE, in_x)],
        json!({"mcp": {"servers": {"github": entry}}}),
    );
    let store = store(&[]);
    let host = dir.start_with(0, |config| wired(config, &store, finds_npx(&npx)));
    let (x, _) = two_matters(&dir, &host);
    host.ok(set_on(AddonKind::Tool, "github", x, false));
    host.shutdown();
    dir.set_policy(&json!({"mcp": {"servers": {"github": entry}}}).to_string());
    let host = dir.start_with(1, |config| wired(config, &store, finds_npx(&npx)));
    assert!(row(&host, Some(x), AddonKind::Tool, "github").on);
    turn(&host, host.create_thread(x, FAKE));
    host.shutdown();
}
