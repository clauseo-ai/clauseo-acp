//! The skills on a real disk, under `CARGO_TARGET_TMPDIR` only, each test's folder removed when
//! it ends: the pure render, the write with its manifest and drift check, other folders and
//! entries left alone, symlinks never followed, and stetdoc's skill from the stub binary
//! (`examples/stetdoc_stub.rs`).

#[path = "skills/write.rs"]
mod write;

use std::path::{Path, PathBuf};

use clauseo_process::{AsyncIoTimer, PassthroughLauncher, SpawnOptions};
use clauseo_profile::skills::manifest::{MANIFEST, sha256_hex};
use clauseo_profile::skills::stetdoc::{self, STETDOC, load_skill};
use clauseo_profile::skills::{self, render, write_rendered};
use clauseo_profile::test_support::tree_text;
use clauseo_profile::{
    BrowserSidecars, ClauseoProfile, ProfilePaths, SkillFile, SkillSource, write_skills,
};
use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;
use futures::executor::block_on;

/// A fresh folder for one test, removed when the test ends.
struct Scratch(PathBuf);

#[cfg(test)]
impl Scratch {
    fn new(name: &str) -> Self {
        let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("clauseo-profile-{name}-{}", std::process::id()));
        write::fresh_dir(&path).unwrap();
        Self(path)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _gone = write::remove_dir_all(&self.0);
    }
}

fn file(path: &str, text: &str) -> SkillFile {
    SkillFile {
        path: path.to_owned(),
        bytes: text.as_bytes().to_vec(),
    }
}

/// A `SKILL.md` in the rendered form, for `name`, with `body`.
fn md(name: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: \"d\"\n---\n{body}")
}

fn skill_md(name: &str, body: &str) -> SkillFile {
    file("SKILL.md", &md(name, body))
}

fn skill(name: &str, version: &str, files: Vec<SkillFile>) -> SkillSource {
    SkillSource::verbatim(name, version, files)
}

#[cfg(test)]
fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[cfg(test)]
fn manifest(root: &Path) -> serde_json::Value {
    serde_json::from_slice(&read(&root.join(MANIFEST))).unwrap()
}

#[test]
fn a_render_writes_the_files_and_their_manifest() {
    let root = Scratch::new("render");
    let rendered = render(&[skill(
        "alpha",
        "1",
        vec![
            skill_md("alpha", "body\n"),
            file("references/a.md", "ref\n"),
        ],
    )])
    .unwrap();
    let report = write_rendered(&root, &rendered).unwrap();
    assert_eq!(report.written.len(), 2);
    let tree = tree_text(&root).replace(&sha256_hex(b"ref\n"), "<ref hash>");
    let tree = tree.replace(
        &sha256_hex(md("alpha", "body\n").as_bytes()),
        "<skill hash>",
    );
    insta::assert_snapshot!(tree);
    let again = write_rendered(&root, &rendered).unwrap();
    assert!(again.written.is_empty() && again.drifted.is_empty());
}

#[test]
fn a_drifted_file_is_reported_and_rewritten() {
    let root = Scratch::new("drift");
    let rendered = render(&[skill("alpha", "1", vec![skill_md("alpha", "one\n")])]).unwrap();
    write_rendered(&root, &rendered).unwrap();
    let target = root.join("alpha").join("SKILL.md");
    write::write(&target, b"onE\n").unwrap();
    let report = write_rendered(&root, &rendered).unwrap();
    assert_eq!(report.drifted, std::slice::from_ref(&target));
    assert_eq!(report.written, std::slice::from_ref(&target));
    assert_eq!(read(&target), md("alpha", "one\n").as_bytes());
}

#[test]
fn a_deleted_file_is_reported_as_drifted_and_rewritten() {
    let root = Scratch::new("deleted");
    let rendered = render(&[skill("alpha", "1", vec![skill_md("alpha", "one\n")])]).unwrap();
    write_rendered(&root, &rendered).unwrap();
    let target = root.join("alpha").join("SKILL.md");
    write::remove_file(&target).unwrap();
    let report = write_rendered(&root, &rendered).unwrap();
    assert_eq!(report.drifted, std::slice::from_ref(&target));
    assert_eq!(read(&target), md("alpha", "one\n").as_bytes());
}

#[test]
fn other_folders_stay_byte_for_byte_and_other_entries_unchanged() {
    let root = Scratch::new("others");
    let stetdoc = render(&[skill(
        STETDOC,
        "stetdoc 0.9.1",
        vec![skill_md(STETDOC, "s\n")],
    )])
    .unwrap();
    write_rendered(&root, &stetdoc).unwrap();
    let lawyers = root.join("lawyers-own").join("SKILL.md");
    write::write(&lawyers, b"mine\n").unwrap();
    let before = (
        manifest(&root)["skills"][STETDOC].clone(),
        read(&root.join(STETDOC).join("SKILL.md")),
    );
    let legal = render(&[skill(
        "legal-drafting",
        "1",
        vec![skill_md("legal-drafting", "l\n")],
    )])
    .unwrap();
    write_rendered(&root, &legal).unwrap();
    write::write(&root.join("legal-drafting").join("SKILL.md"), b"drifted\n").unwrap();
    write_rendered(&root, &legal).unwrap();
    // The entry is compared parsed (the manifest is one file, rewritten whole); the files on
    // disk byte for byte.
    assert_eq!(
        (
            manifest(&root)["skills"][STETDOC].clone(),
            read(&root.join(STETDOC).join("SKILL.md"))
        ),
        before
    );
    assert_eq!(read(&lawyers), b"mine\n");
}

#[test]
fn a_file_the_skill_dropped_is_removed() {
    let root = Scratch::new("removed");
    let two = vec![skill_md("alpha", "a\n"), file("references/old.md", "old\n")];
    write_rendered(&root, &render(&[skill("alpha", "1", two)]).unwrap()).unwrap();
    let one = render(&[skill("alpha", "2", vec![skill_md("alpha", "a\n")])]).unwrap();
    let report = write_rendered(&root, &one).unwrap();
    let old = root.join("alpha").join("references").join("old.md");
    assert_eq!(report.removed, std::slice::from_ref(&old));
    assert!(!old.exists());
}

#[test]
fn a_file_renamed_only_in_case_is_removed_first_then_written() {
    let root = Scratch::new("case");
    let upper = vec![skill_md("alpha", "a\n"), file("Notes.md", "n\n")];
    write_rendered(&root, &render(&[skill("alpha", "1", upper)]).unwrap()).unwrap();
    let lower = vec![skill_md("alpha", "a\n"), file("notes.md", "n\n")];
    let report = write_rendered(&root, &render(&[skill("alpha", "2", lower)]).unwrap()).unwrap();
    assert_eq!(report.removed, [root.join("alpha").join("Notes.md")]);
    // On a disk that ignores case, a removal after the write would delete the new file.
    assert_eq!(read(&root.join("alpha").join("notes.md")), b"n\n");
}

#[test]
fn a_tampered_manifest_never_removes_a_file_outside_the_skill() {
    let root = Scratch::new("tampered");
    let rendered = render(&[skill("alpha", "1", vec![skill_md("alpha", "a\n")])]).unwrap();
    write_rendered(&root, &rendered).unwrap();
    let victim = root.join("victim.md");
    write::write(&victim, b"keep\n").unwrap();
    let mut tampered = manifest(&root);
    for path in ["../victim.md", "./SKILL.md", "/etc/x", "a/../../victim.md"] {
        tampered["skills"]["alpha"]["files"][path] = serde_json::json!("0");
    }
    write::write(
        &root.join(MANIFEST),
        serde_json::to_string(&tampered).unwrap().as_bytes(),
    )
    .unwrap();
    let report = write_rendered(&root, &rendered).unwrap();
    assert!(report.removed.is_empty(), "{report:?}");
    assert_eq!(read(&victim), b"keep\n");
}

#[cfg(unix)]
#[test]
fn a_symlinked_skill_folder_is_never_written_through() {
    let root = Scratch::new("symlink");
    let outside = root.join("outside");
    write::write(&outside.join("old.md"), b"keep\n").unwrap();
    let skills_dir = root.join("skills");
    let two = vec![skill_md("alpha", "a\n"), file("old.md", "old\n")];
    write_rendered(&skills_dir, &render(&[skill("alpha", "1", two)]).unwrap()).unwrap();
    // The skill's folder swapped for a link to a folder outside.
    write::remove_dir_all(&skills_dir.join("alpha")).unwrap();
    write::symlink_dir(&outside, &skills_dir.join("alpha")).unwrap();
    let one = render(&[skill("alpha", "2", vec![skill_md("alpha", "a\n")])]).unwrap();
    let error = write_rendered(&skills_dir, &one).unwrap_err();
    assert!(error.to_string().contains("symlink"), "{error}");
    assert_eq!(read(&outside.join("old.md")), b"keep\n");
    assert!(!outside.join("SKILL.md").exists());
    assert!(!skills::is_current(&skills_dir, "alpha", "1").unwrap());
}

#[test]
fn a_manifest_that_does_not_parse_is_rebuilt_and_unknown_keys_are_kept() {
    let root = Scratch::new("manifest");
    let rendered = render(&[skill("alpha", "1", vec![skill_md("alpha", "a\n")])]).unwrap();
    write::write(&root.join(MANIFEST), b"{ not json").unwrap();
    let report = write_rendered(&root, &rendered).unwrap();
    assert_eq!(report.written.len(), 1);
    assert_eq!(manifest(&root)["format"], 1);
    let mut kept = manifest(&root);
    kept["later"] = serde_json::json!({"x": 1});
    write::write(
        &root.join(MANIFEST),
        serde_json::to_string(&kept).unwrap().as_bytes(),
    )
    .unwrap();
    write_rendered(&root, &rendered).unwrap();
    assert_eq!(manifest(&root)["later"], serde_json::json!({"x": 1}));
}

#[test]
fn a_crashed_writes_temp_files_are_swept_and_other_dot_files_stay() {
    let root = Scratch::new("temps");
    let rendered = render(&[skill("alpha", "1", vec![skill_md("alpha", "a\n")])]).unwrap();
    let leftovers = [
        root.join(".clauseo-manifest.json.123-4.tmp"),
        root.join("alpha").join(".SKILL.md.99-0.tmp"),
    ];
    for leftover in &leftovers {
        write::write(leftover, b"half").unwrap();
    }
    let lawyers = root.join("alpha").join(".notes.tmp");
    write::write(&lawyers, b"mine").unwrap();
    write_rendered(&root, &rendered).unwrap();
    for leftover in &leftovers {
        assert!(!leftover.exists(), "{}", leftover.display());
    }
    assert_eq!(read(&lawyers), b"mine");
}

#[test]
fn concurrent_writes_of_one_folder_all_succeed_with_no_false_drift() {
    let root = Scratch::new("concurrent");
    let paths = profile_paths(&root);
    let (profile, refused) = ClauseoProfile::new(paths.clone(), None).with_skills(&[skill(
        "alpha",
        "1",
        vec![skill_md("alpha", "a\n"), file("references/r.md", "r\n")],
    )]);
    assert!(refused.is_empty());
    let write = profile.skill_files(Some(AgentKind::Claude)).unwrap();
    // Every thread starts before the first join, so the writes overlap.
    let reports: Vec<_> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..8 {
            handles.push(scope.spawn(|| write_skills(&write)));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap().unwrap())
            .collect()
    });
    assert!(reports.iter().all(|report| report.drifted.is_empty()));
    assert_eq!(
        reports
            .iter()
            .map(|report| report.written.len())
            .sum::<usize>(),
        2
    );
    assert!(skills::is_current(&paths.claude_skills(), "alpha", "1").unwrap());
}

#[test]
fn a_bad_skill_is_refused_by_the_render() {
    let refused = [
        skill("Bad_Name", "1", vec![skill_md("Bad_Name", "x")]),
        skill("nul", "1", vec![skill_md("nul", "x")]),
        skill("com1", "1", vec![skill_md("com1", "x")]),
        skill("alpha", "1", vec![skill_md("alpha", "run !`date` now")]),
        skill("alpha", "1", Vec::new()),
        // No SKILL.md, or one with other keys or another name.
        skill("alpha", "1", vec![file("notes.md", "x")]),
        skill(
            "alpha",
            "1",
            vec![file("SKILL.md", "---\nname: alpha\n---\nx")],
        ),
        skill("alpha", "1", vec![skill_md("beta", "x")]),
        skill(
            "alpha",
            "1",
            vec![file(
                "SKILL.md",
                "---\nname: alpha\ndescription: \"d\"\nallowed-tools: Bash\n---\nx",
            )],
        ),
    ];
    for bad in refused {
        assert!(render(std::slice::from_ref(&bad)).is_err(), "{bad:?}");
    }
    let twice = skill("alpha", "1", vec![skill_md("alpha", "x")]);
    assert!(render(&[twice.clone(), twice]).is_err());
}

#[test]
fn a_path_windows_or_a_reader_could_misread_is_refused() {
    for path in [
        "../escape.md",
        "a/./b.md",
        "./SKILL.md",
        "/abs.md",
        "a\\b.md",
        "C:x.md",
        "SKILL.md:stream",
        "trailing.",
        "a /b.md",
        "a b.md",
        ".hidden",
        "con.md",
        "refs/LPT1",
        "a<b>.md",
        "q?.md",
        "",
        "a//b.md",
    ] {
        let bad = skill("alpha", "1", vec![skill_md("alpha", "x"), file(path, "x")]);
        assert!(render(&[bad]).is_err(), "{path:?}");
    }
    let twice_in_case = skill(
        "alpha",
        "1",
        vec![skill_md("alpha", "x"), file("a.md", "1"), file("A.md", "2")],
    );
    assert!(render(&[twice_in_case]).is_err());
    let fine = skill(
        "alpha",
        "1",
        vec![skill_md("alpha", "x"), file("references/a_b-1.v2.md", "x")],
    );
    assert!(render(&[fine]).is_ok());
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
    assert!(render(&[built]).is_ok());
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
    let root = Scratch::new("stetdoc");
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
    let stetdoc = stub(&bin, "stetdoc 0.9.1\n", &md(STETDOC, "first\n"));
    let first = load(&stetdoc);
    assert_eq!(first.version, "stetdoc 0.9.1");
    write_one(&first);
    // Same version: the text comes from the folder, so a changed `skills get core` is not read.
    stub(&bin, "stetdoc 0.9.1\n", &md(STETDOC, "second\n"));
    assert_eq!(
        load(&stetdoc).files[0].bytes,
        md(STETDOC, "first\n").as_bytes()
    );
    // New version: `skills get core` runs again.
    stub(&bin, "stetdoc 0.9.2\n", &md(STETDOC, "second\n"));
    let second = load(&stetdoc);
    assert_eq!(
        (second.version.as_str(), second.files[0].bytes.as_slice()),
        ("stetdoc 0.9.2", md(STETDOC, "second\n").as_bytes())
    );
    write_one(&second);
    assert_eq!(
        read(&skills_dir.join(STETDOC).join("SKILL.md")),
        md(STETDOC, "second\n").as_bytes()
    );
    assert!(skills::is_current(&skills_dir, STETDOC, "stetdoc 0.9.2").unwrap());
}

#[test]
fn stetdocs_frontmatter_is_cut_to_its_name_and_description() {
    let root = Scratch::new("stetdoc-front");
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let text = "---\nname: other\nallowed-tools: Bash(stetdoc:*)\ndescription: Edit Word \
                files.\nhooks:\n  PreToolUse: []\n---\n# stetdoc\n";
    let stetdoc = stub(&root, "stetdoc 0.9.1\n", text);
    let loaded = block_on(load_skill(&launcher, &AsyncIoTimer, &stetdoc, &root)).unwrap();
    assert_eq!(
        String::from_utf8(loaded.files[0].bytes.clone()).unwrap(),
        "---\nname: stetdoc\ndescription: \"Edit Word files.\"\n---\n# stetdoc\n"
    );
    assert!(render(&[loaded]).is_ok());
}

#[test]
fn a_bad_stetdoc_text_fails_its_skill_alone() {
    let root = Scratch::new("stetdoc-bad");
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let stetdoc = stub(&root, "stetdoc 0.9.1\n", &md(STETDOC, "run !`date`\n"));
    let loaded = block_on(load_skill(&launcher, &AsyncIoTimer, &stetdoc, &root)).unwrap();
    let alpha = skill("alpha", "1", vec![skill_md("alpha", "a\n")]);
    let (profile, refused) =
        ClauseoProfile::new(profile_paths(&root), None).with_skills(&[loaded, alpha]);
    assert_eq!(refused.len(), 1, "{refused:?}");
    let write = profile.skill_files(Some(AgentKind::Claude)).unwrap();
    let names: Vec<&str> = write
        .rendered()
        .skills()
        .iter()
        .map(clauseo_profile::RenderedSkill::name)
        .collect();
    assert_eq!(names, ["alpha"]);
    // No description at all: an error from the load, for stetdoc alone.
    stub(&root, "stetdoc 0.9.1\n", "# no frontmatter\n");
    assert!(
        block_on(load_skill(
            &launcher,
            &AsyncIoTimer,
            &stetdoc,
            &root.join("x")
        ))
        .is_err()
    );
}

#[test]
fn a_stetdoc_that_fails_is_an_error() {
    let root = Scratch::new("stetdoc-fails");
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    // The stub with no `stub-version.txt` beside it exits 4.
    let stetdoc = stub(&root, "", "");
    write::remove_file(&root.join("stub-version.txt")).unwrap();
    let error = block_on(load_skill(&launcher, &AsyncIoTimer, &stetdoc, &root)).unwrap_err();
    assert!(error.to_string().contains("exited with Some(4)"), "{error}");
}

#[test]
fn a_stetdoc_past_the_output_cap_is_killed_and_the_cap_reported() {
    let root = Scratch::new("stetdoc-cap");
    let launcher = PassthroughLauncher::new(SpawnOptions::new());
    let stetdoc = stub(&root, "stetdoc 0.9.1\n", &"x".repeat(3 * 1024 * 1024));
    let error = block_on(stetdoc::run(
        &launcher,
        &AsyncIoTimer,
        &stetdoc,
        &["skills", "get", "core"],
    ))
    .unwrap_err();
    assert!(error.to_string().contains("past 1 MiB"), "{error}");
}

#[cfg(test)]
fn profile_paths(root: &Path) -> ProfilePaths {
    ProfilePaths::new(
        &Locations::under_root(root),
        root.join("app").join("clauseo"),
        root.join("stetdoc").join("stetdoc"),
        BrowserSidecars {
            clauseo_mcp: root.join("app").join("clauseo-mcp"),
            agent_browser: root.join("app").join("agent-browser"),
        },
    )
    .unwrap()
}

#[test]
fn claude_gets_the_skills_in_its_plugin_and_codex_none() {
    let root = Scratch::new("agents");
    let paths = profile_paths(&root);
    let (profile, refused) = ClauseoProfile::new(paths.clone(), None).with_skills(&[skill(
        STETDOC,
        "stetdoc 0.9.1",
        vec![skill_md(STETDOC, "s\n")],
    )]);
    assert!(refused.is_empty());
    write_skills(&profile.skill_files(Some(AgentKind::Claude)).unwrap()).unwrap();
    // Codex gets no skills while `CODEX_SKILLS` is off.
    assert!(profile.skill_files(Some(AgentKind::Codex)).is_none());
    let plugin = tree_text(paths.claude_plugin())
        .replace(&sha256_hex(md(STETDOC, "s\n").as_bytes()), "<hash>");
    insta::assert_snapshot!(plugin);
    assert!(!paths.codex_skills().exists());
}
