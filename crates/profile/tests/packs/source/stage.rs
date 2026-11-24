//! A preview staged, found again, expired, swept and installed.

use clauseo_protocol::Date;

use super::{Scratch, put, put_skill, skill_md, zip_of};
use clauseo_profile::packs::source::{
    ADDING_DIR, FromKind, Origin, STAGE_TTL_MS, SourceError, Tree, close_preview, install_picked,
    open_preview, parse_skills_sh, stage_tree, sweep,
};
use clauseo_profile::packs::{
    PACK_MANIFEST, PACKS_DIR, PackEntry, Refusal, Skip, added, load, manifest,
};

const NOW: i64 = 1_800_000_000_000;

fn today() -> Date {
    Date::new(2026, 11, 24).unwrap()
}

/// A GitHub-style archive: one top folder, two skills under `skills/`, one with a script.
fn repo_zip() -> Vec<u8> {
    let pdf = skill_md("pdf");
    let docx = skill_md("docx");
    zip_of(&[
        ("skills-HEAD/README.md", b"# repo"),
        ("skills-HEAD/skills/pdf/SKILL.md", pdf.as_bytes()),
        ("skills-HEAD/skills/pdf/scripts/fill.py", b"print(1)\n"),
        ("skills-HEAD/skills/docx/SKILL.md", docx.as_bytes()),
    ])
}

fn skills_sh_origin() -> Origin {
    let Ok(clauseo_profile::packs::source::Remote::GitHub(repo)) =
        parse_skills_sh("anthropics/skills")
    else {
        panic!("parses");
    };
    repo.origin()
}

#[test]
fn an_archive_is_staged_found_installed_with_its_origin_and_closed() {
    let data = Scratch::new("stage-install");
    let adding = data.path().join(ADDING_DIR);
    let packs = data.path().join(PACKS_DIR);
    let tree = Tree::Zip {
        bytes: &repo_zip(),
        strip_top: true,
        path: None,
    };
    let preview = stage_tree(&adding, tree, skills_sh_origin(), None, NOW).unwrap();
    assert_eq!(preview.token.len(), 32);
    let mut names: Vec<(&str, bool)> = preview
        .skills
        .iter()
        .map(|s| (s.name.as_str(), s.has_scripts))
        .collect();
    names.sort_unstable();
    assert_eq!(names, [("docx", false), ("pdf", true)]);

    let staged = open_preview(&adding, &preview.token, NOW + 1000).unwrap();
    assert_eq!(staged.origin.kind, FromKind::SkillsSh);
    let installed = install_picked(&staged, "pdf", (&packs, &adding), (today(), true), &|_| {
        true
    })
    .unwrap()
    .unwrap();
    assert_eq!(installed.id.as_str(), "pdf");
    assert!(installed.pack.has_scripts());
    let bytes = std::fs::read(packs.join("pdf").join(PACK_MANIFEST)).unwrap();
    let read = manifest::parse(&bytes).unwrap();
    assert!(read.added_here);
    assert_eq!(read.added_from, Some(FromKind::SkillsSh));
    assert_eq!(read.source_url, "https://skills.sh/anthropics/skills");
    assert!(!read.instruction_only);
    assert!(packs.join("pdf/skills/pdf/scripts/fill.py").is_file());

    close_preview(&adding, &preview.token);
    assert!(!adding.join(&preview.token).exists());
    assert_eq!(
        open_preview(&adding, &preview.token, NOW + 2000).map(|_| ()),
        Err(SourceError::Expired)
    );
}

#[test]
fn scripts_are_refused_when_not_allowed_and_the_firm_can_refuse_the_id() {
    let data = Scratch::new("stage-refuse");
    let adding = data.path().join(ADDING_DIR);
    let packs = data.path().join(PACKS_DIR);
    let tree = Tree::Zip {
        bytes: &repo_zip(),
        strip_top: true,
        path: None,
    };
    let preview = stage_tree(&adding, tree, skills_sh_origin(), None, NOW).unwrap();
    let staged = open_preview(&adding, &preview.token, NOW).unwrap();
    let refused = install_picked(&staged, "pdf", (&packs, &adding), (today(), false), &|_| {
        true
    });
    assert!(matches!(refused, Err(added::AddError::Invalid(_))));
    let firm = install_picked(
        &staged,
        "docx",
        (&packs, &adding),
        (today(), false),
        &|_| false,
    );
    assert!(matches!(firm, Ok(None)));
    assert!(!packs.join("docx").exists());
}

#[test]
fn a_path_narrows_and_a_missing_one_is_not_found_leaving_nothing() {
    let data = Scratch::new("stage-narrow");
    let adding = data.path().join(ADDING_DIR);
    let bytes = repo_zip();
    let narrowed = Tree::Zip {
        bytes: &bytes,
        strip_top: true,
        path: Some("skills/docx"),
    };
    let preview = stage_tree(&adding, narrowed, skills_sh_origin(), None, NOW).unwrap();
    assert_eq!(preview.skills.len(), 1);
    assert_eq!(preview.skills[0].name, "docx");
    let missing = Tree::Zip {
        bytes: &bytes,
        strip_top: true,
        path: Some("skills/none"),
    };
    let error = stage_tree(&adding, missing, skills_sh_origin(), None, NOW).map(|_| ());
    assert_eq!(error, Err(SourceError::NotFound));
    let asked = Tree::Zip {
        bytes: &bytes,
        strip_top: true,
        path: None,
    };
    let error = stage_tree(&adding, asked, skills_sh_origin(), Some("xlsx"), NOW).map(|_| ());
    assert_eq!(error, Err(SourceError::NotFound));
    let left: Vec<_> = std::fs::read_dir(&adding).unwrap().collect();
    assert_eq!(left.len(), 1, "only the first preview stays");
}

#[test]
fn a_folder_is_read_in_place_and_previews_expire_and_are_swept() {
    let data = Scratch::new("stage-folder");
    let adding = data.path().join(ADDING_DIR);
    let source = Scratch::new("stage-folder-source");
    put_skill(source.path(), "", "memo");
    let origin = Origin::local(FromKind::Folder, source.path());
    let old = stage_tree(
        &adding,
        Tree::Folder(source.path()),
        origin.clone(),
        None,
        NOW,
    )
    .unwrap();
    let later = NOW + STAGE_TTL_MS;
    let fresh = stage_tree(&adding, Tree::Folder(source.path()), origin, None, later).unwrap();
    assert_eq!(fresh.skills[0].folder, "");
    // Something not named as a token is never swept.
    put(&adding, "memo/clauseo-pack.json", b"{}");

    sweep(&adding, Some(later + 1));
    assert!(!adding.join(&old.token).exists());
    assert!(adding.join(&fresh.token).exists());
    assert_eq!(
        open_preview(&adding, &fresh.token, later + STAGE_TTL_MS + 1).map(|_| ()),
        Err(SourceError::Expired)
    );
    assert!(
        !adding.join(&fresh.token).exists(),
        "an expired preview's folder goes"
    );

    let again = stage_tree(
        &adding,
        Tree::Folder(source.path()),
        Origin::local(FromKind::Folder, source.path()),
        None,
        NOW,
    )
    .unwrap();
    sweep(&adding, None);
    assert!(!adding.join(&again.token).exists());
    assert!(adding.join("memo").exists());
    assert_eq!(
        open_preview(&adding, "../escape", NOW).map(|_| ()),
        Err(SourceError::Expired)
    );
}

#[test]
fn an_old_folder_manifest_still_reads_and_a_bad_origin_is_refused() {
    let old = br#"{"format": 1, "id": "memo", "name": "Memo", "last_reviewed": "2026-10-20",
        "instruction_only": true, "skills": [{"name": "memo"}], "added_here": true}"#;
    let read = manifest::parse(old).unwrap();
    assert_eq!(read.added_from, Some(FromKind::Folder));
    assert_eq!(read.source_url, "");
    let bad = br#"{"format": 1, "id": "memo", "name": "Memo", "last_reviewed": "2026-10-20",
        "instruction_only": true, "skills": [{"name": "memo"}], "added_here": true,
        "added_from": "ftp"}"#;
    assert!(manifest::parse(bad).is_err());
    let plain = br#"{"format": 1, "id": "memo", "name": "Memo", "version": "1", "author": "A",
        "licence": "MIT", "source_url": "https://example.org", "last_reviewed": "2026-10-20",
        "instruction_only": true, "skills": [{"name": "memo"}], "added_from": "zip"}"#;
    assert!(
        manifest::parse(plain).is_err(),
        "added_from belongs to packs added here"
    );
}

#[test]
fn a_pack_added_here_loads_with_its_scripts_only_when_the_entry_allows_it() {
    let data = Scratch::new("stage-launch");
    let adding = data.path().join(ADDING_DIR);
    let packs = data.path().join(PACKS_DIR);
    let tree = Tree::Zip {
        bytes: &repo_zip(),
        strip_top: true,
        path: None,
    };
    let preview = stage_tree(&adding, tree, skills_sh_origin(), None, NOW).unwrap();
    let staged = open_preview(&adding, &preview.token, NOW).unwrap();
    let installed = install_picked(&staged, "pdf", (&packs, &adding), (today(), true), &|_| {
        true
    })
    .unwrap()
    .unwrap();
    let entry = |scripts_if_added| PackEntry {
        id: "pdf".to_owned(),
        hash: installed.pack.hash().clone(),
        added_only: false,
        scripts_if_added,
    };
    let loaded = load(&packs, &[entry(true)], false);
    assert_eq!(loaded.packs.len(), 1, "{:?}", loaded.skipped);
    let refused = load(&packs, &[entry(false)], false);
    assert_eq!(
        refused.skipped,
        [("pdf".to_owned(), Skip::Refused(Refusal::Script))]
    );
    assert_eq!(load(&packs, &[entry(false)], true).packs.len(), 1);
}
