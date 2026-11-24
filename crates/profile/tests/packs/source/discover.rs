//! Finding skills in a tree.

use super::{Scratch, put, put_skill};
use clauseo_profile::packs::Refusal;
use clauseo_profile::packs::source::{SourceError, discover, narrow};

fn names(top: &std::path::Path, asked: Option<&str>) -> Vec<(String, String)> {
    discover(top, asked)
        .unwrap()
        .into_iter()
        .map(|found| (found.folder, found.name))
        .collect()
}

#[test]
fn a_single_skill_repo_is_that_skill_alone() {
    let dir = Scratch::new("single");
    put_skill(dir.path(), "", "pdf");
    put(dir.path(), "references/guide.md", b"# guide");
    put(dir.path(), "notes.txt", b"hello");
    let found = discover(dir.path(), None).unwrap();
    assert_eq!(found.len(), 1);
    let pdf = &found[0];
    assert_eq!(pdf.folder, "");
    assert_eq!(pdf.name, "pdf");
    assert_eq!(pdf.slug.as_deref(), Some("pdf"));
    assert_eq!(pdf.description, "Does pdf things.");
    assert!(!pdf.has_scripts);
    assert!(pdf.problem.is_none());
    // SKILL.md, notes.txt and references/guide.md: the top skill is its whole folder.
    assert_eq!(pdf.files, 3);
}

#[test]
fn a_skills_folder_repo_lists_each_and_left_out_names_are_not_counted() {
    let dir = Scratch::new("skills-dir");
    put_skill(dir.path(), "skills/pdf", "pdf");
    put_skill(dir.path(), "skills/docx", "docx");
    put(dir.path(), "skills/pdf/scripts/fill.py", b"print(1)\n");
    put(dir.path(), "skills/pdf/metadata.json", b"{}");
    put(dir.path(), "skills/pdf/__pycache__/x.pyc", b"\0");
    put(dir.path(), "README.md", b"# repo");
    let found = discover(dir.path(), None).unwrap();
    let docx = found.iter().find(|f| f.name == "docx").unwrap();
    let pdf = found.iter().find(|f| f.name == "pdf").unwrap();
    assert_eq!(found.len(), 2);
    assert!(!docx.has_scripts);
    assert!(pdf.has_scripts);
    assert_eq!(pdf.files, 2);
    assert_eq!(pdf.folder, "skills/pdf");
}

#[test]
fn nested_containers_are_found_and_a_shallower_skill_shadows_deeper_ones() {
    let dir = Scratch::new("nested");
    put_skill(dir.path(), "skills/legal/contracts/nda", "nda");
    put_skill(dir.path(), "skills/.curated/memo", "memo");
    put_skill(dir.path(), ".claude/skills/brief", "brief");
    put_skill(dir.path(), "skills/outer", "outer");
    put_skill(dir.path(), "skills/outer/inner", "inner");
    // Four levels under skills/: past the container depth, and not looked for when others exist.
    put_skill(dir.path(), "skills/a/b/c/deep", "deep");
    let mut got = names(dir.path(), None);
    got.sort();
    assert_eq!(
        got,
        [
            (".claude/skills/brief".to_owned(), "brief".to_owned()),
            ("skills/.curated/memo".to_owned(), "memo".to_owned()),
            ("skills/legal/contracts/nda".to_owned(), "nda".to_owned()),
            ("skills/outer".to_owned(), "outer".to_owned()),
        ]
    );
}

#[test]
fn the_fallback_search_finds_a_skill_anywhere_shallow_enough() {
    let dir = Scratch::new("fallback");
    put_skill(dir.path(), "a/b/c/found", "found");
    put_skill(dir.path(), "a/b/c/d/e/f/too-deep", "too-deep");
    put_skill(dir.path(), "node_modules/pkg/skipped", "skipped");
    assert_eq!(
        names(dir.path(), None),
        [("a/b/c/found".to_owned(), "found".to_owned())]
    );
}

#[test]
fn skills_sh_json_paths_are_searched() {
    let dir = Scratch::new("config");
    put(
        dir.path(),
        "skills.sh.json",
        br#"{"paths": ["./tools/skills/", "../outside", "/abs", "no-dot"]}"#,
    );
    put_skill(dir.path(), "tools/skills/tax", "tax");
    put_skill(dir.path(), "skills/pdf", "pdf");
    let mut got = names(dir.path(), None);
    got.sort();
    assert_eq!(
        got,
        [
            ("skills/pdf".to_owned(), "pdf".to_owned()),
            ("tools/skills/tax".to_owned(), "tax".to_owned()),
        ]
    );
}

#[test]
fn internal_skills_are_hidden_unless_asked_for_and_names_narrow() {
    let dir = Scratch::new("internal");
    put(
        dir.path(),
        "skills/secret/SKILL.md",
        b"---\nname: secret\ndescription: Hidden.\nmetadata:\n  internal: true\n---\nBody\n",
    );
    put_skill(dir.path(), "skills/pdf", "pdf");
    put_skill(dir.path(), "skills/review_contract", "Review Contract");
    assert_eq!(names(dir.path(), None).len(), 2);
    assert_eq!(names(dir.path(), Some("secret")).len(), 1);
    assert_eq!(names(dir.path(), Some("review-contract")).len(), 1);
    assert_eq!(
        discover(dir.path(), Some("nope")),
        Err(SourceError::NotFound)
    );
    let all = discover(dir.path(), None).unwrap();
    assert_eq!(narrow(all, "PDF").len(), 1);
}

#[test]
fn a_skill_without_name_or_description_is_no_skill() {
    let dir = Scratch::new("bad-head");
    put(
        dir.path(),
        "skills/x/SKILL.md",
        b"---\nname: x\n---\nBody\n",
    );
    put(dir.path(), "skills/y/SKILL.md", b"no frontmatter at all");
    assert_eq!(discover(dir.path(), None), Err(SourceError::NotFound));
}

#[test]
fn a_skill_over_the_cap_is_reported_too_big() {
    let dir = Scratch::new("too-big");
    put_skill(dir.path(), "skills/big", "big");
    for n in 0..201 {
        put(dir.path(), &format!("skills/big/refs/r{n}.md"), b"x");
    }
    put_skill(dir.path(), "skills/small", "small");
    let found = discover(dir.path(), None).unwrap();
    let big = found.iter().find(|f| f.name == "big").unwrap();
    assert_eq!(big.problem, Some(Refusal::TooBig));
    assert!(big.files > 200);
    let small = found.iter().find(|f| f.name == "small").unwrap();
    assert_eq!(small.problem, None);
}

#[test]
fn a_skill_breaking_a_pack_rule_is_reported_with_its_refusal() {
    let dir = Scratch::new("bad-path");
    put_skill(dir.path(), "skills/spaced", "spaced");
    put(dir.path(), "skills/spaced/a file.md", b"x");
    let found = discover(dir.path(), None).unwrap();
    assert_eq!(found[0].problem, Some(Refusal::BadPath));
}

#[cfg(unix)]
#[test]
fn symlinks_are_never_followed() {
    let dir = Scratch::new("symlink");
    let outside = Scratch::new("symlink-outside");
    put_skill(outside.path(), "", "outside");
    put_skill(dir.path(), "skills/real", "real");
    super::symlink(outside.path(), &dir.path().join("skills/linked"));
    super::symlink(
        &outside.path().join("SKILL.md"),
        &dir.path().join("skills/real/extra.md"),
    );
    let found = discover(dir.path(), None).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "real");
    assert_eq!(found[0].files, 1);
}

#[cfg(unix)]
#[test]
fn an_executable_file_counts_as_a_script() {
    let dir = Scratch::new("exec");
    put_skill(dir.path(), "", "exec");
    put(dir.path(), "notes.md", b"plain");
    super::set_mode(&dir.path().join("notes.md"), 0o755);
    assert!(discover(dir.path(), None).unwrap()[0].has_scripts);
}
