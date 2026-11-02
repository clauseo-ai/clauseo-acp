//! The skills on a real disk, under `CARGO_TARGET_TMPDIR` only: the pure render, the write with
//! its manifest and drift check, other folders and entries left alone, and stetdoc's skill from
//! the stub binary (`examples/stetdoc_stub.rs`).

#[path = "skills/write.rs"]
mod write;

use std::path::{Path, PathBuf};

use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use clauseo_profile::skills::manifest::{MANIFEST, sha256_hex};
use clauseo_profile::skills::stetdoc::{STETDOC, load_skill};
use clauseo_profile::skills::{self, render, write_rendered};
use clauseo_profile::test_support::tree_text;
use clauseo_profile::{
    BrowserSidecars, ClauseoProfile, ProfilePaths, SkillFile, SkillSource, write_skills,
};
use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;
use futures::executor::block_on;

/// A fresh folder for one test.
#[cfg(test)]
fn scratch(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("clauseo-profile-{name}-{}", std::process::id()));
    write::fresh_dir(&path).unwrap();
    path
}

fn file(path: &str, text: &str) -> SkillFile {
    SkillFile {
        path: path.to_owned(),
        bytes: text.as_bytes().to_vec(),
    }
}

fn skill(name: &str, version: &str, files: Vec<SkillFile>) -> SkillSource {
    SkillSource::verbatim(name, version, files)
}

#[cfg(test)]
fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn a_render_writes_the_files_and_their_manifest() {
    let root = scratch("render");
    let rendered = render(&[skill(
        "alpha",
        "1",
        vec![
            file("SKILL.md", "---\nname: alpha\n---\nbody\n"),
            file("references/a.md", "ref\n"),
        ],
    )])
    .unwrap();
    let report = write_rendered(&root, &rendered).unwrap();
    assert_eq!(report.written.len(), 2);
    let tree = tree_text(&root).replace(&sha256_hex(b"ref\n"), "<ref hash>");
    let tree = tree.replace(
        &sha256_hex(b"---\nname: alpha\n---\nbody\n"),
        "<skill hash>",
    );
    insta::assert_snapshot!(tree);
    let again = write_rendered(&root, &rendered).unwrap();
    assert!(again.written.is_empty() && again.drifted.is_empty());
}

#[test]
fn a_drifted_file_is_reported_and_rewritten() {
    let root = scratch("drift");
    let rendered = render(&[skill("alpha", "1", vec![file("SKILL.md", "one\n")])]).unwrap();
    write_rendered(&root, &rendered).unwrap();
    let target = root.join("alpha").join("SKILL.md");
    write::write(&target, b"onE\n").unwrap();
    let report = write_rendered(&root, &rendered).unwrap();
    assert_eq!(report.drifted, std::slice::from_ref(&target));
    assert_eq!(report.written, std::slice::from_ref(&target));
    assert_eq!(read(&target), b"one\n");
}

#[test]
fn other_folders_and_entries_stay_byte_for_byte() {
    let root = scratch("others");
    let stetdoc = render(&[skill(
        STETDOC,
        "stetdoc 0.9.1",
        vec![file("SKILL.md", "s\n")],
    )])
    .unwrap();
    write_rendered(&root, &stetdoc).unwrap();
    let lawyers = root.join("lawyers-own").join("SKILL.md");
    write::write(&lawyers, b"mine\n").unwrap();
    let stetdoc_entry = |root: &Path| {
        let manifest: serde_json::Value =
            serde_json::from_slice(&read(&root.join(MANIFEST))).unwrap();
        manifest["skills"][STETDOC].clone()
    };
    let before = (
        stetdoc_entry(&root),
        read(&root.join(STETDOC).join("SKILL.md")),
    );
    let legal = render(&[skill("legal-drafting", "1", vec![file("SKILL.md", "l\n")])]).unwrap();
    write_rendered(&root, &legal).unwrap();
    write::write(&root.join("legal-drafting").join("SKILL.md"), b"drifted\n").unwrap();
    write_rendered(&root, &legal).unwrap();
    assert_eq!(
        (
            stetdoc_entry(&root),
            read(&root.join(STETDOC).join("SKILL.md"))
        ),
        before
    );
    assert_eq!(read(&lawyers), b"mine\n");
}

#[test]
fn a_file_the_skill_dropped_is_removed() {
    let root = scratch("removed");
    let two = vec![file("SKILL.md", "a\n"), file("references/old.md", "old\n")];
    write_rendered(&root, &render(&[skill("alpha", "1", two)]).unwrap()).unwrap();
    let one = render(&[skill("alpha", "2", vec![file("SKILL.md", "a\n")])]).unwrap();
    let report = write_rendered(&root, &one).unwrap();
    let old = root.join("alpha").join("references").join("old.md");
    assert_eq!(report.removed, std::slice::from_ref(&old));
    assert!(!old.exists());
}

#[test]
fn a_bad_skill_is_refused_before_anything_is_written() {
    let refused = [
        skill("Bad_Name", "1", vec![file("SKILL.md", "x")]),
        skill("alpha", "1", vec![file("../escape.md", "x")]),
        skill("alpha", "1", vec![file("SKILL.md", "run !`date` now")]),
        skill("alpha", "1", Vec::new()),
    ];
    for bad in refused {
        assert!(render(std::slice::from_ref(&bad)).is_err(), "{bad:?}");
    }
    let twice = skill("alpha", "1", vec![file("SKILL.md", "x")]);
    assert!(render(&[twice.clone(), twice]).is_err());
}

#[test]
fn a_builtin_skill_is_rendered_with_name_and_description_alone() {
    let source = "---\nname: matter-brief\ndescription: Keeps the \"brief\".\nmetadata:\n  \
                  clauseo:\n    version: 3\n    author: Clauseo\n    tools: []\n    tier: \
                  builtin\n---\n# Matter brief\n";
    let built = SkillSource::builtin("matter-brief", vec![file("SKILL.md", source)]).unwrap();
    assert_eq!(built.version, "3");
    assert_eq!(
        String::from_utf8(built.files[0].bytes.clone()).unwrap(),
        "---\nname: matter-brief\ndescription: \"Keeps the \\\"brief\\\".\"\n---\n# Matter brief\n"
    );
    assert!(SkillSource::builtin("other-name", vec![file("SKILL.md", source)]).is_err());
}

/// The stub copied into `dir` with what it prints.
#[cfg(test)]
fn stub(dir: &Path, version: &str, skill: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let examples = exe.parent().unwrap().parent().unwrap().join("examples");
    let name = format!("stetdoc_stub{}", std::env::consts::EXE_SUFFIX);
    let target = dir.join(&name);
    if !target.exists() {
        write::copy(&examples.join(&name), &target).unwrap();
    }
    write::write(&dir.join("stub-version.txt"), version.as_bytes()).unwrap();
    write::write(&dir.join("stub-skill.md"), skill.as_bytes()).unwrap();
    target
}

#[test]
fn stetdocs_skill_is_regenerated_when_its_version_changes() {
    let root = scratch("stetdoc");
    let bin = root.join("bin");
    write::fresh_dir(&bin).unwrap();
    let skills_dir = root.join("skills");
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let load = |stetdoc: &Path| {
        block_on(load_skill(&launcher, &AsyncIoTimer, stetdoc, &skills_dir)).unwrap()
    };
    let write_one = |source: &SkillSource| {
        let rendered = render(std::slice::from_ref(source)).unwrap();
        write_rendered(&skills_dir, &rendered).unwrap();
    };
    let stetdoc = stub(&bin, "stetdoc 0.9.1\n", "first\n");
    let first = load(&stetdoc);
    assert_eq!(first.version, "stetdoc 0.9.1");
    write_one(&first);
    // Same version: the text comes from the folder, so a changed `skills get core` is not read.
    stub(&bin, "stetdoc 0.9.1\n", "second\n");
    assert_eq!(load(&stetdoc).files[0].bytes, b"first\n");
    // New version: `skills get core` runs again.
    stub(&bin, "stetdoc 0.9.2\n", "second\n");
    let second = load(&stetdoc);
    assert_eq!(
        (second.version.as_str(), second.files[0].bytes.as_slice()),
        ("stetdoc 0.9.2", b"second\n".as_slice())
    );
    write_one(&second);
    assert_eq!(
        read(&skills_dir.join(STETDOC).join("SKILL.md")),
        b"second\n"
    );
    assert!(skills::is_current(&skills_dir, STETDOC, "stetdoc 0.9.2").unwrap());
}

#[test]
fn a_stetdoc_that_fails_is_an_error() {
    let root = scratch("stetdoc-fails");
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    // The stub with no `stub-version.txt` beside it exits 4.
    let stetdoc = stub(&root, "", "");
    write::remove_file(&root.join("stub-version.txt")).unwrap();
    let error = block_on(load_skill(&launcher, &AsyncIoTimer, &stetdoc, &root)).unwrap_err();
    assert!(error.to_string().contains("exited with Some(4)"), "{error}");
}

#[test]
fn claude_gets_the_skills_in_its_plugin_and_codex_none() {
    let root = scratch("agents");
    let paths = ProfilePaths::new(
        &Locations::under_root(&root),
        root.join("stetdoc"),
        BrowserSidecars {
            clauseo_mcp: root.join("clauseo-mcp"),
            agent_browser: root.join("agent-browser"),
        },
    );
    let profile = ClauseoProfile::new(paths.clone(), None)
        .with_skills(&[skill(
            STETDOC,
            "stetdoc 0.9.1",
            vec![file("SKILL.md", "s\n")],
        )])
        .unwrap();
    write_skills(&profile.skill_files(Some(AgentKind::Claude)).unwrap()).unwrap();
    // Codex gets no skills while `CODEX_SKILLS` is off.
    assert!(profile.skill_files(Some(AgentKind::Codex)).is_none());
    let plugin = tree_text(&paths.claude_plugin).replace(&sha256_hex(b"s\n"), "<hash>");
    insta::assert_snapshot!(plugin);
    assert!(!paths.codex_skills.exists());
}
