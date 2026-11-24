//! `SetAddonScope`, `SetAddonOn` and `RemoveAddon` at the host's interface (decision record
//! 0040, Scope and per-matter switches), read back through `ListAddons { matter }`: a scope
//! `matter`, a matter's off switch, an item turned on from a matter, a removal clearing the
//! switches, the built-ins' and unknown ids' refusals, a firm's lock, and the added tools'
//! `server`. What a launch gets in each matter is in `scope_launch.rs`.

use std::sync::Arc;

use clauseo_connectors::MemoryStore;
use clauseo_protocol::{
    Addon, AddonKind, AddonScope, ErrorCode, HostCommand, HostQuery, MatterId, McpSecretName,
    QueryResult, RemoveAddon, SetAddonOn, SetAddonScope,
};
use serde_json::{Value, json};

use super::item;
use crate::mcp_tools::{HELD, account, local, save, set, store};
use crate::packs::{put_pack, skills_on};
use crate::support::{Dir, Running};
use crate::write;

/// The list as it reads for `matter`.
pub(crate) fn listed(host: &Running, matter: Option<MatterId>) -> Vec<Addon> {
    match host.query(HostQuery::ListAddons { matter }).unwrap() {
        QueryResult::Addons(list) => list.items,
        other => panic!("ListAddons answered {other:?}"),
    }
}

/// The row of `kind` with `id` as it reads for `matter`.
pub(crate) fn row(host: &Running, matter: Option<MatterId>, kind: AddonKind, id: &str) -> Addon {
    item(&listed(host, matter), kind, id).clone()
}

/// Whether `id` of `kind` is listed at all.
fn is_listed(host: &Running, kind: AddonKind, id: &str) -> bool {
    listed(host, None)
        .iter()
        .any(|item| item.kind == kind && item.id == id)
}

pub(crate) fn scope(kind: AddonKind, id: &str, scope: AddonScope) -> HostCommand {
    HostCommand::SetAddonScope(SetAddonScope {
        kind,
        id: id.to_owned(),
        scope,
    })
}

pub(crate) fn set_on(kind: AddonKind, id: &str, matter: MatterId, on: bool) -> HostCommand {
    HostCommand::SetAddonOn(SetAddonOn {
        kind,
        id: id.to_owned(),
        matter,
        on,
    })
}

fn remove(kind: AddonKind, id: &str) -> HostCommand {
    HostCommand::RemoveAddon(RemoveAddon {
        kind,
        id: id.to_owned(),
    })
}

/// Two matters, X and Y, each on a folder of its own.
pub(crate) fn two_matters(dir: &Dir, host: &Running) -> (MatterId, MatterId) {
    let x = host.create_matter("x", &dir.folder("x", &[("brief.txt", "Clause 4\n")]));
    let y = host.create_matter("y", &dir.folder("y", &[("brief.txt", "Clause 9\n")]));
    (x, y)
}

/// The settings file as JSON.
fn settings_file(dir: &Dir) -> Value {
    serde_json::from_str(&dir.settings_json()).unwrap()
}

/// A pack turned on for every matter, given scope `matter` X: on in X, off in Y, both rows
/// saying scope X and switched `[X]`; the settings record the scope. Scope `all` again clears
/// the switch: on in Y, switched `[]`.
#[test]
fn a_pack_scoped_to_a_matter_is_on_there_only() {
    let dir = Dir::new("addons_scope_matter", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    dir.set_settings(&[], skills_on(&[("nda-kit", &hash)]));
    let host = dir.start(0);
    let (x, y) = two_matters(&dir, &host);
    host.ok(scope(AddonKind::Skill, "nda-kit", AddonScope::Matter(x)));
    let in_x = row(&host, Some(x), AddonKind::Skill, "nda-kit");
    assert!(in_x.on, "{in_x:?}");
    assert_eq!(in_x.scope, AddonScope::Matter(x));
    assert_eq!(in_x.switched, [x]);
    let in_y = row(&host, Some(y), AddonKind::Skill, "nda-kit");
    assert!(!in_y.on, "{in_y:?}");
    assert_eq!(in_y.scope, AddonScope::Matter(x));
    assert_eq!(in_y.switched, [x]);
    assert_eq!(settings_file(&dir)["skills"]["scope"]["nda-kit"], "matter");

    host.ok(scope(AddonKind::Skill, "nda-kit", AddonScope::All));
    let in_y = row(&host, Some(y), AddonKind::Skill, "nda-kit");
    assert!(in_y.on, "{in_y:?}");
    assert_eq!(in_y.scope, AddonScope::All);
    assert!(in_y.switched.is_empty(), "{in_y:?}");
    assert!(row(&host, Some(x), AddonKind::Skill, "nda-kit").on);
    host.shutdown();
}

/// An `all` pack and server switched off in X: off in X, on in Y and with no matter, each
/// switched `[X]` with scope `all`. Switching it on again in X stores no switch.
#[test]
fn an_off_switch_turns_an_item_off_in_one_matter() {
    let dir = Dir::new("addons_scope_off", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let mut extra = skills_on(&[("nda-kit", &hash)]);
    extra["mcp"] = json!({"servers": {"github": {"command": "npx", "enabled": true}}});
    dir.set_settings(&[], extra);
    let host = dir.start(0);
    let (x, y) = two_matters(&dir, &host);
    for (kind, id) in [(AddonKind::Skill, "nda-kit"), (AddonKind::Tool, "github")] {
        host.ok(set_on(kind, id, x, false));
        let in_x = row(&host, Some(x), kind, id);
        assert!(!in_x.on, "{in_x:?}");
        assert_eq!(
            (in_x.scope, in_x.switched.as_slice()),
            (AddonScope::All, &[x][..])
        );
        let in_y = row(&host, Some(y), kind, id);
        assert!(in_y.on, "{in_y:?}");
        assert_eq!(in_y.switched, [x]);
        assert!(row(&host, None, kind, id).on, "{id} with no matter");

        host.ok(set_on(kind, id, x, true));
        let in_x = row(&host, Some(x), kind, id);
        assert!(in_x.on && in_x.switched.is_empty(), "{in_x:?}");
    }
    host.shutdown();
}

/// `SetAddonOn` true in X for a pack not turned on and a server not enabled turns each on with
/// scope `matter`, on in X only.
#[test]
fn turning_on_an_item_that_is_on_nowhere_puts_it_in_that_matter_only() {
    let dir = Dir::new("addons_scope_promote", &[]);
    put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let mut extra = skills_on(&[]);
    extra["mcp"] = json!({"servers": {"github": {"command": "npx", "enabled": false}}});
    dir.set_settings(&[], extra);
    let host = dir.start(0);
    let (x, y) = two_matters(&dir, &host);
    assert!(!row(&host, Some(x), AddonKind::Skill, "nda-kit").on);
    assert!(!row(&host, Some(x), AddonKind::Tool, "github").on);
    for (kind, id) in [(AddonKind::Skill, "nda-kit"), (AddonKind::Tool, "github")] {
        host.ok(set_on(kind, id, x, true));
        let in_x = row(&host, Some(x), kind, id);
        assert!(in_x.on, "{in_x:?}");
        assert_eq!(in_x.scope, AddonScope::Matter(x));
        assert_eq!(in_x.switched, [x]);
        let in_y = row(&host, Some(y), kind, id);
        assert!(!in_y.on, "{in_y:?}");
        assert_eq!(in_y.switched, [x]);
    }
    let file = settings_file(&dir);
    assert!(file["skills"]["enabled"].get("nda-kit").is_some(), "{file}");
    assert_eq!(file["skills"]["scope"]["nda-kit"], "matter", "{file}");
    assert_eq!(file["mcp"]["servers"]["github"]["enabled"], true, "{file}");
    assert_eq!(
        file["mcp"]["servers"]["github"]["scope"], "matter",
        "{file}"
    );
    host.shutdown();
}

/// A pack added on this computer and scoped to X, removed: it is no longer listed, and added
/// again it is `all` with no switch. A server switched off in X, removed: gone, its keychain
/// item deleted, and saved again it is on in X with no switch.
#[test]
fn removing_an_item_clears_its_switches() {
    let dir = Dir::new("addons_scope_remove", &[]);
    let source = dir.data().with_file_name("source-skill");
    let skill = "---\nname: nda-review\ndescription: Reviews an NDA.\n---\n# NDA\n";
    write::write(&source.join("SKILL.md"), skill.as_bytes());
    dir.set_settings(&[], json!({"skills": {"extra": "any"}}));
    let store = store(&[]);
    let host = dir.start_with(0, |config| {
        config.with_mcp_secrets(Arc::<MemoryStore>::clone(&store))
    });
    let (x, y) = two_matters(&dir, &host);
    let add = || HostCommand::AddSkill {
        path: source.to_str().unwrap().to_owned(),
    };
    host.ok(add());
    host.ok(scope(AddonKind::Skill, "nda-review", AddonScope::Matter(x)));
    assert!(!row(&host, Some(y), AddonKind::Skill, "nda-review").on);
    host.ok(remove(AddonKind::Skill, "nda-review"));
    assert!(!is_listed(&host, AddonKind::Skill, "nda-review"));
    host.ok(add());
    let again = row(&host, Some(y), AddonKind::Skill, "nda-review");
    assert!(again.on, "{again:?}");
    assert_eq!((again.scope, again.switched.len()), (AddonScope::All, 0));

    host.ok(save(None, local("github", "npx", vec![set("TOKEN", HELD)])));
    host.ok(set_on(AddonKind::Tool, "github", x, false));
    assert_eq!(row(&host, None, AddonKind::Tool, "github").switched, [x]);
    host.ok(remove(AddonKind::Tool, "github"));
    assert!(!is_listed(&host, AddonKind::Tool, "github"));
    // One command's keychain work runs at a time: this save's answer means the removal's
    // delete has ended.
    host.ok(save(None, local("other", "npx", Vec::new())));
    assert_eq!(store.peek(&account("github", "TOKEN")), None);
    host.ok(save(None, local("github", "npx", Vec::new())));
    let back = row(&host, Some(x), AddonKind::Tool, "github");
    assert!(back.on && back.switched.is_empty(), "{back:?}");
    host.shutdown();
}

/// Every change to a built-in skill is `addon.built_in`, as are `browser`'s scope and
/// `case-law`'s removal; `case-law` switched off in X is off there, switched `[X]`, and on
/// again stores no switch.
#[test]
fn built_ins_take_only_a_tools_matter_switch() {
    let dir = Dir::new("addons_scope_built_in", &[]);
    let host = dir.start(0);
    let (x, y) = two_matters(&dir, &host);
    let skill = AddonKind::Skill;
    for refused in [
        set_on(skill, "legal-drafting", x, false),
        set_on(skill, "matter-brief", x, true),
        scope(skill, "legal-research", AddonScope::Matter(x)),
        scope(skill, "stetdoc", AddonScope::All),
        remove(skill, "agent-browser"),
        scope(AddonKind::Tool, "browser", AddonScope::Matter(x)),
        scope(AddonKind::Tool, "case-law", AddonScope::All),
        remove(AddonKind::Tool, "case-law"),
        remove(AddonKind::Tool, "browser"),
    ] {
        host.refused(refused, ErrorCode::AddonBuiltIn);
    }
    let unswitched = row(&host, None, AddonKind::Tool, "case-law");
    host.ok(set_on(AddonKind::Tool, "case-law", x, false));
    let in_x = row(&host, Some(x), AddonKind::Tool, "case-law");
    assert!(!in_x.on, "{in_x:?}");
    assert_eq!(
        (in_x.scope, in_x.switched.as_slice()),
        (AddonScope::All, &[x][..])
    );
    let in_y = row(&host, Some(y), AddonKind::Tool, "case-law");
    assert_eq!(
        (in_y.on, in_y.switched.as_slice()),
        (unswitched.on, &[x][..])
    );
    host.ok(set_on(AddonKind::Tool, "case-law", x, true));
    assert_eq!(
        row(&host, Some(x), AddonKind::Tool, "case-law").switched,
        Vec::<MatterId>::new()
    );
    host.shutdown();
}

/// A skill or tool id nobody has is `addon.unknown` for each command, and changes nothing.
#[test]
fn an_unknown_id_is_refused() {
    let dir = Dir::new("addons_scope_unknown", &[]);
    dir.set_settings(&[], json!({"skills": {"extra": "any"}}));
    let host = dir.start(0);
    let (x, _) = two_matters(&dir, &host);
    let before = dir.settings_json();
    let (skill, tool) = (AddonKind::Skill, AddonKind::Tool);
    for refused in [
        set_on(skill, "ghost-kit", x, true),
        set_on(skill, "ghost-kit", x, false),
        scope(skill, "ghost-kit", AddonScope::All),
        scope(skill, "ghost-kit", AddonScope::Matter(x)),
        remove(skill, "ghost-kit"),
        set_on(skill, "Not A Pack", x, true),
        set_on(tool, "ghost", x, true),
        set_on(tool, "ghost", x, false),
        scope(tool, "ghost", AddonScope::Matter(x)),
        remove(tool, "ghost"),
    ] {
        host.refused(refused, ErrorCode::AddonUnknown);
    }
    assert_eq!(dir.settings_json(), before);
    assert!(!is_listed(&host, skill, "ghost-kit"));
    host.shutdown();
}

/// A firm policy that sets `skills.enabled` locks every skill change: `settings.locked`.
#[test]
fn a_firm_lock_on_skills_enabled_refuses_every_skill_change() {
    let dir = Dir::new("addons_scope_locked", &[]);
    put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let host = dir.start_with_policy(0, r#"{"skills": {"extra": "any", "enabled": {}}}"#);
    let (x, _) = two_matters(&dir, &host);
    for refused in [
        set_on(AddonKind::Skill, "nda-kit", x, true),
        set_on(AddonKind::Skill, "nda-kit", x, false),
        scope(AddonKind::Skill, "nda-kit", AddonScope::Matter(x)),
        remove(AddonKind::Skill, "nda-kit"),
    ] {
        host.refused(refused, ErrorCode::SettingsLocked);
    }
    let row = row(&host, Some(x), AddonKind::Skill, "nda-kit");
    assert!(row.locked && !row.on && row.switched.is_empty(), "{row:?}");
    host.shutdown();
}

/// An added tool's row carries `server` (names and `saved` flags, never a value); a skill's
/// and a built-in's do not. The test prints the answer for X between markers.
#[test]
fn added_tools_carry_their_server_and_the_answer_is_printed() {
    let dir = Dir::new("addons_scope_server", &[]);
    let hash = put_pack(&dir, "nda-kit", &["nda-review"], b"Notes.\n");
    let mut extra = skills_on(&[("nda-kit", &hash)]);
    extra["mcp"] = json!({"servers": {
        "github": {"command": "npx", "args": ["-y", "server-github"],
            "env": {"TOKEN": {"keychain": true}, "REGION": "uk"}, "enabled": true},
        "linear": {"url": "https://mcp.linear.app/mcp", "enabled": true},
    }});
    dir.set_settings(&[], extra);
    let store = store(&[(&account("github", "TOKEN"), HELD)]);
    let host = dir.start_with(0, |config| {
        config.with_mcp_secrets(Arc::<MemoryStore>::clone(&store))
    });
    let (x, _) = two_matters(&dir, &host);
    host.ok(scope(AddonKind::Skill, "nda-kit", AddonScope::Matter(x)));
    host.ok(set_on(AddonKind::Tool, "github", x, false));
    host.ok(set_on(AddonKind::Tool, "case-law", x, false));
    let items = listed(&host, Some(x));
    let github = item(&items, AddonKind::Tool, "github");
    let server = github.server.as_ref().expect("github's server");
    assert_eq!(server.name, "github");
    assert_eq!(server.command.as_deref(), Some("npx"));
    assert_eq!(server.args, ["-y", "server-github"]);
    assert_eq!(
        server.env,
        ["REGION", "TOKEN"].map(|name| McpSecretName {
            name: name.to_owned(),
            saved: true,
        })
    );
    assert!(server.enabled && !github.on, "{github:?}");
    let linear = item(&items, AddonKind::Tool, "linear");
    let remote = linear.server.as_ref().expect("linear's server");
    assert_eq!(remote.url.as_deref(), Some("https://mcp.linear.app/mcp"));
    assert!(
        remote.env.is_empty() && remote.headers.is_empty(),
        "{remote:?}"
    );
    for other in items
        .iter()
        .filter(|item| item.id != "github" && item.id != "linear")
    {
        assert_eq!(other.server, None, "{other:?}");
    }
    let answer =
        serde_json::to_string_pretty(&QueryResult::Addons(clauseo_protocol::AddonList { items }))
            .unwrap();
    assert!(
        !answer.contains(HELD),
        "the answer holds the keychain value"
    );
    assert!(
        !answer.contains("\"uk\""),
        "the answer holds a variable's value"
    );
    println!("D2A-LIST-BEGIN\n{answer}\nD2A-LIST-END");
    host.shutdown();
}
