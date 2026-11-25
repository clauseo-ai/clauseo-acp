//! `SetAddonOn` with no matter (decision record 0040): off makes the item's scope `matter` with
//! every switch cleared, so it is on in no matter; on is `SetAddonScope` `all`. Each is one
//! commit, then `AddonsChanged`; under the firm's lock it is `settings.locked` where a scope
//! change is; Clauseo's own tools refuse it. And `SaveServer` with a `scope`: a server added for
//! X goes to X's launch only, an edit with none keeps the scope it had, and one with the scope it
//! already has changes no switch.

use clauseo_protocol::{AddonKind, AddonScope, ErrorCode};
use serde_json::json;

use super::scope::{everywhere, row, set_on, two_matters};
use super::scope_review_launch::{programs, server_env, still_scoped, with_servers};
use super::{addons_changed, drain};
use crate::mcp_tools::{local, save, store, wired};
use crate::packs::put_pack;
use crate::profile_rig::turn;
use crate::support::{Dir, FAKE};

/// A second generic agent, whose scenario expects Y's launch.
const OTHER: &str = "fake-agent-y";

/// A pack and a server, each switched off in X first: off everywhere leaves each on in no
/// matter (scope `matter`, no switch), then on everywhere puts each in every matter (scope
/// `all`, no switch). Each answers done, then `AddonsChanged`.
#[test]
fn off_and_on_everywhere_are_one_change_each() {
    let dir = Dir::new("addons_everywhere", &[]);
    let hash = put_pack(&dir, "lit-kit", &["lit-review"], b"Notes.\n");
    dir.set_settings(
        &[],
        json!({"skills": {"extra": "any", "enabled": {"lit-kit": hash}},
            "mcp": {"servers": {"linear": {"command": "npx", "enabled": true}}}}),
    );
    let host = dir.start(0);
    let (x, y) = two_matters(&dir, &host);
    let (mut sub, _first) = crate::settings::subscribe_app(&host);
    for (kind, id) in [(AddonKind::Skill, "lit-kit"), (AddonKind::Tool, "linear")] {
        host.ok(set_on(kind, id, x, false));
        drain(&host, &mut sub);
        host.ok(everywhere(kind, id, false));
        addons_changed(&mut sub);
        for matter in [x, y] {
            let off = row(&host, Some(matter), kind, id);
            assert!(!off.on && off.switched.is_empty(), "{off:?}");
            assert!(matches!(off.scope, AddonScope::Matter(_)), "{off:?}");
        }
        drain(&host, &mut sub);
        host.ok(everywhere(kind, id, true));
        addons_changed(&mut sub);
        for matter in [x, y] {
            let on = row(&host, Some(matter), kind, id);
            assert!(on.on && on.switched.is_empty(), "{on:?}");
            assert_eq!(on.scope, AddonScope::All);
        }
    }
    let file: serde_json::Value = serde_json::from_str(&dir.settings_json()).unwrap();
    assert!(
        file["mcp"]["servers"]["linear"]["enabled"] == true,
        "{file}"
    );
    host.shutdown();
}

/// Under the firm's lock, every matter at once is `settings.locked` either way, even for an
/// item the firm scopes to a matter; Clauseo's own tools refuse it with `addon.built_in`.
#[test]
fn every_matter_at_once_is_refused_under_the_lock_and_for_built_ins() {
    let dir = Dir::new("addons_everywhere_locked", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let policy = json!({"skills": {"extra": "any", "enabled": {"nda-kit": hash},
            "scope": {"nda-kit": "matter"}},
        "mcp": {"servers": {"github": {"command": "npx", "enabled": true, "scope": "matter"}}}});
    let host = dir.start_with_policy(0, &policy.to_string());
    for (kind, id) in [(AddonKind::Skill, "nda-kit"), (AddonKind::Tool, "github")] {
        for on in [false, true] {
            host.refused(everywhere(kind, id, on), ErrorCode::SettingsLocked);
        }
    }
    for id in ["browser", "case-law"] {
        host.refused(
            everywhere(AddonKind::Tool, id, false),
            ErrorCode::AddonBuiltIn,
        );
    }
    host.shutdown();
}

/// A server added through `SaveServer` with scope X: X's `session/new` carries it, Y's none
/// (each scenario checks `mcpServers` exactly); then an edit with no scope keeps it X's.
#[test]
fn a_server_saved_with_a_scope_goes_to_that_matter_only() {
    let dir = Dir::new("addons_save_scope", &[]);
    let (npx, _, resolver) = programs(&dir);
    let server = json!([{"name": "github", "command": npx.display().to_string(),
        "args": ["-y", "server-github"], "env": server_env(&npx, Vec::new())}]);
    let in_x = with_servers(&dir, "save-scope-x", &server);
    let in_y = with_servers(&dir, "save-scope-y", &json!([]));
    dir.set_settings(&[(FAKE, in_x), (OTHER, in_y)], json!({}));
    let store = store(&[]);
    let host = dir.start_with(0, |config| wired(config, &store, resolver));
    let (x, y) = two_matters(&dir, &host);
    let mut added = local("github", "npx", Vec::new());
    added.scope = Some(AddonScope::Matter(x));
    host.ok(save(None, added));
    still_scoped(&dir, &host, (x, y));
    host.ok(save(Some("github"), local("github", "npx", Vec::new())));
    still_scoped(&dir, &host, (x, y));
    turn(&host, host.create_thread(x, FAKE));
    turn(&host, host.create_thread(y, OTHER));
    host.shutdown();
}

/// A server switched off in X, then edited with the scope it already has (`all`, as an edit
/// form sends it back) and again with none: it stays off in X, and X's `session/new` lacks it
/// while Y's carries it.
#[test]
fn an_edit_with_the_scope_it_has_keeps_every_switch() {
    let dir = Dir::new("addons_save_same_scope", &[]);
    let (npx, _, resolver) = programs(&dir);
    let server = json!([{"name": "github", "command": npx.display().to_string(),
        "args": ["-y", "server-github"], "env": server_env(&npx, Vec::new())}]);
    let in_x = with_servers(&dir, "same-scope-x", &json!([]));
    let in_y = with_servers(&dir, "same-scope-y", &server);
    dir.set_settings(
        &[(FAKE, in_x), (OTHER, in_y)],
        json!({"mcp": {"servers": {"github": {"command": "npx",
            "args": ["-y", "server-github"], "enabled": true}}}}),
    );
    let store = store(&[]);
    let host = dir.start_with(0, |config| wired(config, &store, resolver));
    let (x, y) = two_matters(&dir, &host);
    host.ok(set_on(AddonKind::Tool, "github", x, false));
    let mut same = local("github", "npx", Vec::new());
    same.scope = Some(AddonScope::All);
    for edit in [same, local("github", "npx", Vec::new())] {
        host.ok(save(Some("github"), edit));
        let in_x = row(&host, Some(x), AddonKind::Tool, "github");
        assert!(!in_x.on && in_x.switched == [x], "{in_x:?}");
        assert_eq!(in_x.scope, AddonScope::All);
        assert!(row(&host, Some(y), AddonKind::Tool, "github").on);
    }
    turn(&host, host.create_thread(x, FAKE));
    turn(&host, host.create_thread(y, OTHER));
    host.shutdown();
}
