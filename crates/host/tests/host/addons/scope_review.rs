//! Scopes and per-matter switches after review (decision record 0040), at the host's
//! interface: under a firm's lock on `skills.enabled` or `mcp.servers` a matter's switch is
//! left only to the items the firm scopes to a matter; Settings › Tools' removal and rename and
//! `RemoveSkill` clear or carry every matter's switch; and a forgotten matter comes back with
//! none. What a launch gets after a server's edit, and with Clauseo's own tools switched off in
//! a matter, is in `scope_review_launch.rs`.

use std::sync::Arc;

use clauseo_connectors::MemoryStore;
use clauseo_protocol::{
    AddonKind, AddonScope, ErrorCode, HostCommand, MatterId, McpCommand, PackId, RemoveAddon,
};
use serde_json::{Value, json};

use super::scope::{listed, row, scope, set_on, two_matters};
use crate::mcp_tools::{local, save, store};
use crate::packs::put_pack;
use crate::support::{Dir, Running};
use crate::write;

fn remove(kind: AddonKind, id: &str) -> HostCommand {
    HostCommand::RemoveAddon(RemoveAddon {
        kind,
        id: id.to_owned(),
    })
}

/// The settings file as JSON.
fn settings_file(dir: &Dir) -> Value {
    serde_json::from_str(&dir.settings_json()).unwrap()
}

/// Whether `id` of `kind` is listed at all.
fn is_listed(host: &Running, kind: AddonKind, id: &str) -> bool {
    listed(host, None)
        .iter()
        .any(|item| item.kind == kind && item.id == id)
}

/// A host under a policy that sets `skills.enabled` and `skills.scope`: `nda-kit` on with
/// scope `matter`, `lit-kit` on with scope `all`, and `spare-kit` on disk but not in the
/// firm's list.
fn skills_locked(dir: &Dir) -> Running {
    let scoped = put_pack(dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let everywhere = put_pack(dir, "lit-kit", &["lit-review"], b"Notes.\n");
    put_pack(dir, "spare-kit", &["spare-review"], b"Notes.\n");
    let policy = json!({"skills": {"extra": "any",
        "enabled": {"nda-kit": scoped, "lit-kit": everywhere},
        "scope": {"nda-kit": "matter"}}});
    dir.start_with_policy(0, &policy.to_string())
}

/// A host under a policy that sets `mcp.servers`: `github` enabled with scope `matter`,
/// `linear` enabled with scope `all`, `jira` in the list but disabled. The lawyer's own
/// `mine`, in the user's settings, is not in the firm's list.
fn tools_locked(dir: &Dir) -> Running {
    dir.set_settings(
        &[],
        json!({"mcp": {"servers": {"mine": {"command": "npx", "enabled": true}}}}),
    );
    let policy = json!({"mcp": {"servers": {
        "github": {"command": "npx", "enabled": true, "scope": "matter"},
        "linear": {"command": "npx", "enabled": true},
        "jira": {"command": "npx", "enabled": false},
    }}});
    dir.start_with_policy(0, &policy.to_string())
}

/// Switches `id` on in X and off again, each reflected in X's list and never in Y's; then its
/// scope and its removal are `settings.locked`.
fn switches_under_the_lock(
    host: &Running,
    (x, y): (MatterId, MatterId),
    kind: AddonKind,
    id: &str,
) {
    let before = row(host, Some(x), kind, id);
    assert!(!before.on && before.switched.is_empty(), "{before:?}");
    host.ok(set_on(kind, id, x, true));
    let in_x = row(host, Some(x), kind, id);
    assert!(in_x.on, "{in_x:?}");
    assert_eq!(in_x.switched, [x]);
    let in_y = row(host, Some(y), kind, id);
    assert!(!in_y.on, "{in_y:?}");
    host.ok(set_on(kind, id, x, false));
    let off = row(host, Some(x), kind, id);
    assert!(!off.on && off.switched.is_empty(), "{off:?}");
    for refused in [
        scope(kind, id, AddonScope::All),
        scope(kind, id, AddonScope::Matter(y)),
        remove(kind, id),
    ] {
        host.refused(refused, ErrorCode::SettingsLocked);
    }
    assert!(!row(host, Some(y), kind, id).on);
}

/// Under the lock, a pack the firm turns on with scope `matter` still takes a matter's switch:
/// on in X alone, then off again. Its scope and its removal stay `settings.locked`.
#[test]
fn a_pack_the_firm_scopes_to_a_matter_takes_a_switch_under_the_lock() {
    let dir = Dir::new("addons_review_lock_skill_matter", &[]);
    let host = skills_locked(&dir);
    let matters = two_matters(&dir, &host);
    switches_under_the_lock(&host, matters, AddonKind::Skill, "nda-kit");
    host.shutdown();
}

/// Under the lock, a server the firm enables with scope `matter` still takes a matter's switch:
/// on in X alone, then off again. Its scope and its removal stay `settings.locked`.
#[test]
fn a_server_the_firm_scopes_to_a_matter_takes_a_switch_under_the_lock() {
    let dir = Dir::new("addons_review_lock_tool_matter", &[]);
    let host = tools_locked(&dir);
    let matters = two_matters(&dir, &host);
    switches_under_the_lock(&host, matters, AddonKind::Tool, "github");
    host.shutdown();
}

/// Refuses every change to `id` with `settings.locked`, and leaves it on in X and Y with no
/// switch.
fn locked_on_everywhere(host: &Running, (x, y): (MatterId, MatterId), kind: AddonKind, id: &str) {
    for refused in [
        set_on(kind, id, x, false),
        set_on(kind, id, x, true),
        scope(kind, id, AddonScope::Matter(x)),
        remove(kind, id),
    ] {
        host.refused(refused, ErrorCode::SettingsLocked);
    }
    for matter in [x, y] {
        let item = row(host, Some(matter), kind, id);
        assert!(item.on && item.switched.is_empty(), "{item:?}");
        assert_eq!(item.scope, AddonScope::All);
    }
}

/// Under the lock, a pack the firm turns on for every matter takes no switch:
/// `settings.locked`, and it stays on in every matter.
#[test]
fn a_pack_the_firm_turns_on_everywhere_takes_no_switch() {
    let dir = Dir::new("addons_review_lock_skill_all", &[]);
    let host = skills_locked(&dir);
    let matters = two_matters(&dir, &host);
    locked_on_everywhere(&host, matters, AddonKind::Skill, "lit-kit");
    host.shutdown();
}

/// Under the lock, a server the firm enables for every matter takes no switch:
/// `settings.locked`, and it stays on in every matter.
#[test]
fn a_server_the_firm_enables_everywhere_takes_no_switch() {
    let dir = Dir::new("addons_review_lock_tool_all", &[]);
    let host = tools_locked(&dir);
    let matters = two_matters(&dir, &host);
    locked_on_everywhere(&host, matters, AddonKind::Tool, "linear");
    host.shutdown();
}

/// Under the lock, a pack on disk that the firm does not turn on cannot be turned on in a
/// matter, nor switched, scoped or removed: `settings.locked`, and it stays off.
#[test]
fn a_pack_the_firm_leaves_out_cannot_be_turned_on_in_a_matter() {
    let dir = Dir::new("addons_review_lock_skill_unlisted", &[]);
    let host = skills_locked(&dir);
    let (x, y) = two_matters(&dir, &host);
    let skill = AddonKind::Skill;
    for refused in [
        set_on(skill, "spare-kit", x, true),
        set_on(skill, "spare-kit", x, false),
        scope(skill, "spare-kit", AddonScope::Matter(x)),
        remove(skill, "spare-kit"),
    ] {
        host.refused(refused, ErrorCode::SettingsLocked);
    }
    for matter in [x, y] {
        let item = row(&host, Some(matter), skill, "spare-kit");
        assert!(!item.on && item.switched.is_empty(), "{item:?}");
    }
    host.shutdown();
}

/// Under the lock, the lawyer's own server is not in the firm's list, so it is no server at all
/// (`addon.unknown`); a server the firm lists disabled cannot be turned on in a matter, nor
/// scoped or removed (`settings.locked`), and stays off.
#[test]
fn a_server_the_firm_leaves_out_cannot_be_turned_on_in_a_matter() {
    let dir = Dir::new("addons_review_lock_tool_unlisted", &[]);
    let host = tools_locked(&dir);
    let (x, y) = two_matters(&dir, &host);
    let tool = AddonKind::Tool;
    assert!(!is_listed(&host, tool, "mine"));
    host.refused(set_on(tool, "mine", x, true), ErrorCode::AddonUnknown);
    for refused in [
        set_on(tool, "jira", x, true),
        set_on(tool, "jira", x, false),
        scope(tool, "jira", AddonScope::Matter(x)),
        remove(tool, "jira"),
    ] {
        host.refused(refused, ErrorCode::SettingsLocked);
    }
    for matter in [x, y] {
        let item = row(&host, Some(matter), tool, "jira");
        assert!(!item.on && item.switched.is_empty(), "{item:?}");
    }
    host.shutdown();
}

/// A host with `servers` as `mcp.servers` and a memory keychain.
fn with_servers(dir: &Dir, servers: &Value) -> Running {
    dir.set_settings(&[], json!({"mcp": {"servers": servers}}));
    let store: Arc<MemoryStore> = store(&[]);
    dir.start_with(0, move |config| config.with_mcp_secrets(store))
}

/// Settings › Tools' `RemoveServer` clears every matter's switch for the server: switched off
/// in X, removed, and saved again under the same name, it is on in X with no switch.
#[test]
fn removing_a_server_in_settings_clears_every_matters_switch() {
    let dir = Dir::new("addons_review_remove_server", &[]);
    let host = with_servers(
        &dir,
        &json!({"github": {"command": "npx", "enabled": true}}),
    );
    let (x, y) = two_matters(&dir, &host);
    host.ok(set_on(AddonKind::Tool, "github", x, false));
    assert_eq!(row(&host, None, AddonKind::Tool, "github").switched, [x]);
    host.ok(HostCommand::Mcp(McpCommand::RemoveServer {
        name: "github".to_owned(),
    }));
    assert!(!is_listed(&host, AddonKind::Tool, "github"));
    host.ok(save(None, local("github", "npx", Vec::new())));
    let back = row(&host, Some(x), AddonKind::Tool, "github");
    assert!(back.on && back.switched.is_empty(), "{back:?}");
    assert_eq!(back.scope, AddonScope::All);
    assert!(row(&host, Some(y), AddonKind::Tool, "github").on);
    host.shutdown();
}

/// A rename through Settings › Tools' `SaveServer` carries the server's matters: one scoped to X
/// keeps scope X and switched `[X]` under its new name (and `matter` in its settings entry); one
/// switched off in Y stays off there under its new name. The old names are gone.
#[test]
fn renaming_a_server_keeps_its_scope_and_its_switches() {
    let dir = Dir::new("addons_review_rename_server", &[]);
    let host = with_servers(
        &dir,
        &json!({"github": {"command": "npx", "enabled": true},
            "linear": {"command": "npx", "enabled": true}}),
    );
    let (x, y) = two_matters(&dir, &host);
    let tool = AddonKind::Tool;
    host.ok(scope(tool, "github", AddonScope::Matter(x)));
    host.ok(set_on(tool, "linear", y, false));
    host.ok(save(Some("github"), local("hub", "npx", Vec::new())));
    host.ok(save(Some("linear"), local("lin", "npx", Vec::new())));
    assert!(!is_listed(&host, tool, "github") && !is_listed(&host, tool, "linear"));

    let hub = row(&host, Some(x), tool, "hub");
    assert!(hub.on, "{hub:?}");
    assert_eq!(hub.scope, AddonScope::Matter(x));
    assert_eq!(hub.switched, [x]);
    assert!(!row(&host, Some(y), tool, "hub").on);
    let lin = row(&host, Some(y), tool, "lin");
    assert!(!lin.on, "{lin:?}");
    assert_eq!(
        (lin.scope, lin.switched.as_slice()),
        (AddonScope::All, &[y][..])
    );
    assert!(row(&host, Some(x), tool, "lin").on);

    let file = settings_file(&dir);
    let servers = &file["mcp"]["servers"];
    assert_eq!(servers["hub"]["scope"], "matter", "{file}");
    assert!(
        servers.get("github").is_none() && servers.get("linear").is_none(),
        "{file}"
    );
    host.shutdown();
}

/// `RemoveSkill` of a skill added on this computer and switched off in X clears that switch:
/// added again under the same id, it is on in X with no switch.
#[test]
fn remove_skill_clears_every_matters_switch() {
    let dir = Dir::new("addons_review_remove_skill", &[]);
    let source = dir.data().with_file_name("source-skill");
    let skill = "---\nname: nda-review\ndescription: Reviews an NDA.\n---\n# NDA\n";
    write::write(&source.join("SKILL.md"), skill.as_bytes());
    dir.set_settings(&[], json!({"skills": {"extra": "any"}}));
    let host = dir.start(0);
    let (x, y) = two_matters(&dir, &host);
    let add = || HostCommand::AddSkill {
        path: source.to_str().unwrap().to_owned(),
    };
    host.ok(add());
    host.ok(set_on(AddonKind::Skill, "nda-review", x, false));
    let off = row(&host, Some(x), AddonKind::Skill, "nda-review");
    assert!(!off.on, "{off:?}");
    assert_eq!(off.switched, [x]);
    host.ok(HostCommand::RemoveSkill {
        id: PackId::new("nda-review").unwrap(),
    });
    assert!(!is_listed(&host, AddonKind::Skill, "nda-review"));
    host.ok(add());
    let again = row(&host, Some(x), AddonKind::Skill, "nda-review");
    assert!(again.on && again.switched.is_empty(), "{again:?}");
    assert!(row(&host, Some(y), AddonKind::Skill, "nda-review").on);
    host.shutdown();
}

/// A matter forgotten with a server and case law switched off in it comes back (its folder
/// imported again, same run) with no switch, and still none after a restart.
#[test]
fn a_forgotten_matter_comes_back_with_no_switch() {
    let dir = Dir::new("addons_review_forget", &[]);
    dir.set_settings(
        &[],
        json!({"mcp": {"servers": {"github": {"command": "npx", "enabled": true}}}}),
    );
    let host = dir.start(0);
    let folder = dir.folder("x", &[("brief.txt", "Clause 4\n")]);
    let x = host.create_matter("x", &folder);
    for id in ["github", "case-law"] {
        host.ok(set_on(AddonKind::Tool, id, x, false));
        assert_eq!(row(&host, Some(x), AddonKind::Tool, id).switched, [x]);
    }
    host.ok(HostCommand::ForgetMatter { matter: x });
    assert_eq!(
        host.create_matter("x", &folder),
        x,
        "the same matter comes back"
    );
    let unswitched = |host: &Running| {
        for id in ["github", "case-law"] {
            let back = row(host, Some(x), AddonKind::Tool, id);
            assert!(back.switched.is_empty(), "{back:?}");
        }
        assert!(row(host, Some(x), AddonKind::Tool, "github").on);
    };
    unswitched(&host);
    host.shutdown();
    let host = dir.start(1);
    unswitched(&host);
    host.shutdown();
}
