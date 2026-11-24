//! `ListAddons`' skills: the built-ins with their descriptions, the packs' scripts,
//! descriptions and states, a firm's lock, and `AddonsChanged` after a pack is turned on.

use clauseo_protocol::{
    AddonFrom, AddonFromKind, AddonKind, AddonScope, AddonSource, AddonStatus, HostCommand,
    HostQuery, MatterId, PackId, QueryResult,
};

use super::{addons, addons_changed, drain, item};
use crate::support::{Dir, foreign_uuid};
use crate::write;

/// Writes a valid pack `id` with one skill `review` described by `description`, and `extra`
/// files beside its `SKILL.md`.
fn put_pack(dir: &Dir, id: &str, description: &str, extra: &[(&str, &[u8])]) {
    let root = dir.data().join("skill-packs").join(id);
    let manifest = serde_json::json!({
        "format": 1,
        "id": id,
        "name": format!("{id} pack"),
        "version": "1.0.0",
        "author": "Example Chambers",
        "source_url": "https://example.org/pack",
        "licence": "CC-BY-4.0",
        "last_reviewed": "2026-10-20",
        "instruction_only": extra.is_empty(),
        "skills": [{"name": "review"}],
    });
    write::write(
        &root.join("clauseo-pack.json"),
        manifest.to_string().as_bytes(),
    );
    let skill = format!("---\nname: review\ndescription: {description}\n---\n# Review\n");
    write::write(&root.join("skills/review/SKILL.md"), skill.as_bytes());
    for (path, bytes) in extra {
        write::write(&root.join("skills/review").join(path), bytes);
    }
}

fn set_pack(id: &str, on: bool) -> HostCommand {
    HostCommand::SetSkillPack {
        id: PackId::new(id).unwrap(),
        on,
    }
}

/// With no pack and no server: the three built-in skills, each with its fixed line, then
/// `case-law` (listed though this install has no data connectors: `not_offered`) and `browser`;
/// every row `all`, none removable. The test prints the answer as the wire carries it.
#[test]
fn the_built_ins_are_listed_with_descriptions() {
    let dir = Dir::new("addons_built_ins", &[]);
    let host = dir.start(0);
    let items = addons(&host);
    let ids: Vec<(AddonKind, &str)> = items
        .iter()
        .map(|item| (item.kind, item.id.as_str()))
        .collect();
    assert_eq!(
        ids,
        [
            (AddonKind::Skill, "legal-drafting"),
            (AddonKind::Skill, "legal-research"),
            (AddonKind::Skill, "matter-brief"),
            (AddonKind::Tool, "case-law"),
            (AddonKind::Tool, "browser"),
        ]
    );
    let drafting = item(&items, AddonKind::Skill, "legal-drafting");
    assert_eq!(
        drafting.description,
        "Drafting or revising a legal document, or marking up a draft"
    );
    for item in &items {
        assert_eq!(item.source, AddonSource::BuiltIn, "{item:?}");
        assert_eq!(item.scope, AddonScope::All, "{item:?}");
        assert!(!item.can_remove && !item.description.is_empty(), "{item:?}");
        assert_eq!(item.from, None, "{item:?}");
        let scripts = (item.kind == AddonKind::Skill).then_some(false);
        assert_eq!(item.has_scripts, scripts, "{item:?}");
    }
    let case_law = item(&items, AddonKind::Tool, "case-law");
    assert_eq!(case_law.name, "Case law (Indian Kanoon)");
    assert_eq!(case_law.status, AddonStatus::NotOffered);
    // A matter changes nothing in this version.
    let matter = MatterId::new(foreign_uuid(1)).unwrap();
    let QueryResult::Addons(with_matter) = host
        .query(HostQuery::ListAddons {
            matter: Some(matter),
        })
        .unwrap()
    else {
        panic!("ListAddons answered another result");
    };
    assert_eq!(with_matter.items, items);
    let answer = QueryResult::Addons(with_matter);
    println!(
        "list_addons answer:\n{}",
        serde_json::to_string_pretty(&answer).unwrap()
    );
}

/// A pack holding a script has `has_scripts`, a Markdown-only one not; each takes its first
/// skill's description, cut to the index's 200 characters; one turned on is `on` and `ready`,
/// and turning it on sends `AddonsChanged`; one whose files changed since is `changed`.
#[test]
fn packs_show_their_scripts_descriptions_and_states() {
    let dir = Dir::new("addons_packs", &[]);
    let long = "word ".repeat(30);
    put_pack(
        &dir,
        "plain-kit",
        "Reviews a lease.",
        &[("notes.md", b"# Notes\n")],
    );
    put_pack(
        &dir,
        "script-kit",
        long.trim(),
        &[("run.py", b"print(1)\n")],
    );
    dir.set_settings(
        &[],
        serde_json::json!({"skills": {"extra": "any", "allow_scripts": true}}),
    );
    let host = dir.start(0);
    let (mut sub, _first) = crate::settings::subscribe_app(&host);
    drain(&host, &mut sub);
    host.ok(set_pack("plain-kit", true));
    addons_changed(&mut sub);
    let items = addons(&host);
    let plain = item(&items, AddonKind::Skill, "plain-kit");
    assert_eq!(plain.has_scripts, Some(false), "{plain:?}");
    assert_eq!(plain.description, "Reviews a lease.");
    assert_eq!((plain.on, plain.status), (true, AddonStatus::Ready));
    assert_eq!(plain.source, AddonSource::Added);
    assert_eq!(
        plain.from,
        Some(AddonFrom {
            kind: AddonFromKind::Url,
            text: "https://example.org/pack".to_owned()
        })
    );
    assert!(!plain.can_remove && !plain.locked, "{plain:?}");
    let script = item(&items, AddonKind::Skill, "script-kit");
    assert_eq!(script.has_scripts, Some(true), "{script:?}");
    assert_eq!((script.on, script.status), (false, AddonStatus::Ready));
    assert!(script.description.chars().count() <= 200, "{script:?}");
    assert!(script.description.ends_with('…'), "{script:?}");
    // The added skills follow the built-in ones, by id.
    let skills: Vec<&str> = items
        .iter()
        .filter(|item| item.kind == AddonKind::Skill)
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(skills[3..], ["plain-kit", "script-kit"]);

    write::write(
        &dir.data()
            .join("skill-packs/plain-kit/skills/review/notes.md"),
        b"# Notes, edited\n",
    );
    let changed = addons(&host);
    let plain = item(&changed, AddonKind::Skill, "plain-kit");
    assert_eq!((plain.on, plain.status), (false, AddonStatus::Changed));
}

/// A firm policy that sets `skills.enabled` locks every pack's row, and one that sets
/// `tools.browser` locks the browser's; a pack the firm's curated list leaves out is
/// `blocked`.
#[test]
fn a_firm_lock_shows_as_locked() {
    let dir = Dir::new("addons_locked", &[]);
    put_pack(&dir, "nda-kit", "Reviews an NDA.", &[]);
    put_pack(&dir, "lease-kit", "Reviews a lease.", &[]);
    let policy = r#"{"skills": {"extra": "curated", "curated": ["lease-kit"], "enabled": {}},
        "tools": {"browser": false}}"#;
    let host = dir.start_with_policy(0, policy);
    let items = addons(&host);
    let nda = item(&items, AddonKind::Skill, "nda-kit");
    assert!(nda.locked, "{nda:?}");
    assert_eq!(nda.status, AddonStatus::Blocked);
    let lease = item(&items, AddonKind::Skill, "lease-kit");
    assert!(lease.locked, "{lease:?}");
    assert_eq!((lease.on, lease.status), (false, AddonStatus::Ready));
    let browser = item(&items, AddonKind::Tool, "browser");
    assert!(browser.locked && !browser.on, "{browser:?}");
    assert!(!item(&items, AddonKind::Tool, "case-law").locked);
    assert!(!item(&items, AddonKind::Skill, "legal-drafting").locked);
}
