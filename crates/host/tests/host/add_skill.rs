//! `AddSkill` and `RemoveSkill` (decision record 0040): a skill from this computer made a pack
//! and turned on, replaced when added again, removed, and admitted with `skills.extra` off.

use std::path::{Path, PathBuf};

use clauseo_protocol::{
    ErrorCode, HostCommand, HostQuery, PackId, PackState, PackStatus, QueryResult,
};

use crate::support::{Dir, Running};
use crate::write;

/// A vendor's `SKILL.md`, with keys the profile does not take.
const VENDOR: &str = "---\nname: NDA Review\ndescription: Reviews an NDA.\nallowed-tools: Bash\nlicense: MIT\n---\n# NDA\n";

fn source(dir: &Dir) -> PathBuf {
    dir.data().with_file_name("source-skill")
}

fn start(dir: &Dir, extra: &str) -> Running {
    dir.set_settings(&[], serde_json::json!({ "skills": {"extra": extra} }));
    dir.start(0)
}

fn add(path: &Path) -> HostCommand {
    HostCommand::AddSkill {
        path: path.to_str().unwrap().to_owned(),
    }
}

fn list(host: &Running) -> Vec<PackStatus> {
    match host.query(HostQuery::ListSkillPacks).unwrap() {
        QueryResult::SkillPacks(packs) => packs,
        other => panic!("ListSkillPacks answered {other:?}"),
    }
}

fn enabled(dir: &Dir) -> serde_json::Value {
    let settings: serde_json::Value = serde_json::from_str(&dir.settings_json()).unwrap();
    settings["skills"]["enabled"].clone()
}

fn skill_md(dir: &Dir) -> String {
    let path = dir
        .data()
        .join("skill-packs/nda-review/skills/nda-review/SKILL.md");
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn a_folder_is_added_on_with_its_frontmatter_cut() {
    let dir = Dir::new("add_skill_folder", &[]);
    write::write(&source(&dir).join("SKILL.md"), VENDOR.as_bytes());
    write::write(&source(&dir).join("forms/notice.md"), b"# Notice\n");
    let host = start(&dir, "any");
    host.ok(add(&source(&dir)));
    let listed = list(&host);
    let pack = listed
        .iter()
        .find(|p| p.id.as_str() == "nda-review")
        .unwrap();
    assert_eq!(pack.state, PackState::On, "{pack:?}");
    assert!(pack.added_here);
    assert_eq!(pack.name.as_deref(), Some("NDA Review"));
    assert_eq!(pack.source_url, None);
    assert!(enabled(&dir).get("nda-review").is_some());
    assert_eq!(
        skill_md(&dir),
        "---\nname: nda-review\ndescription: \"Reviews an NDA.\"\n---\n# NDA\n"
    );
    assert!(
        dir.data()
            .join("skill-packs/nda-review/skills/nda-review/forms/notice.md")
            .is_file()
    );
}

#[test]
fn a_lone_skill_md_is_added() {
    let dir = Dir::new("add_skill_file", &[]);
    write::write(&source(&dir).join("SKILL.md"), VENDOR.as_bytes());
    write::write(&source(&dir).join("other.md"), b"# Not copied\n");
    let host = start(&dir, "any");
    host.ok(add(&source(&dir).join("SKILL.md")));
    assert_eq!(list(&host)[0].state, PackState::On);
    assert!(
        !dir.data()
            .join("skill-packs/nda-review/skills/nda-review/other.md")
            .exists()
    );
}

#[test]
fn adding_again_replaces_it() {
    let dir = Dir::new("add_skill_again", &[]);
    write::write(&source(&dir).join("SKILL.md"), VENDOR.as_bytes());
    let host = start(&dir, "any");
    host.ok(add(&source(&dir)));
    let first = enabled(&dir)["nda-review"].clone();
    write::write(
        &source(&dir).join("SKILL.md"),
        VENDOR.replace("# NDA", "# NDA v2").as_bytes(),
    );
    host.ok(add(&source(&dir)));
    assert!(skill_md(&dir).ends_with("# NDA v2\n"));
    assert_ne!(enabled(&dir)["nda-review"], first);
    let listed = list(&host);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].state, PackState::On);
}

#[test]
fn removing_deletes_the_folder_and_the_entry() {
    let dir = Dir::new("add_skill_remove", &[]);
    write::write(&source(&dir).join("SKILL.md"), VENDOR.as_bytes());
    let host = start(&dir, "any");
    host.ok(add(&source(&dir)));
    let id = PackId::new("nda-review").unwrap();
    host.ok(HostCommand::RemoveSkill { id: id.clone() });
    assert!(!dir.data().join("skill-packs/nda-review").exists());
    assert!(enabled(&dir).get("nda-review").is_none());
    assert_eq!(list(&host), []);
    host.refused(HostCommand::RemoveSkill { id }, ErrorCode::SettingsRefused);
}

#[test]
fn an_added_skill_is_admitted_with_the_lawyers_extra_off() {
    let dir = Dir::new("add_skill_extra_off", &[]);
    write::write(&source(&dir).join("SKILL.md"), VENDOR.as_bytes());
    let host = start(&dir, "off");
    host.ok(add(&source(&dir)));
    let listed = list(&host);
    assert_eq!(listed[0].state, PackState::On, "{listed:?}");
    assert!(listed[0].added_here);
}

#[test]
fn a_path_with_no_skill_md_is_refused() {
    let dir = Dir::new("add_skill_none", &[]);
    write::write(&source(&dir).join("README.md"), b"# Nothing\n");
    let host = start(&dir, "any");
    host.refused(add(&source(&dir)), ErrorCode::SkillNoSkillMd);
    assert_eq!(list(&host), []);
}
