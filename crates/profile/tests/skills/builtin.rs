//! R22's three built-in skills on a real disk: the render snapshots for Claude and Codex, the
//! drift check, `stetdoc` and every other folder left alone, and the Codex index's paths, each
//! exactly where the write puts the file.

use std::path::Path;

use clauseo_profile::skills::builtin::{BUILTIN, RETIRED_BUILTINS, sources};
use clauseo_profile::skills::manifest::MANIFEST;
use clauseo_profile::skills::stetdoc::{STETDOC, skill_source};
use clauseo_profile::skills::{SHELL_INJECTION, SKILL_MD, render, retire, write_rendered};
use clauseo_profile::test_support::tree_text;
use clauseo_profile::{ClauseoProfile, SkillFile, SkillSource, write_skills};
use clauseo_protocol::AgentKind;

use super::{Scratch, manifest, profile_paths, read, write};

/// Every file under `root`, by path, with its bytes.
#[cfg(test)]
fn files_under(root: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            let bytes = read(&path);
            out.push((path, bytes));
        }
    }
    out.sort();
    out
}

/// stetdoc's skill as R21 renders it.
#[cfg(test)]
fn stetdoc() -> clauseo_profile::SkillSource {
    skill_source(
        "stetdoc 0.9.1",
        b"---\ndescription: Edit Word files as tracked changes.\n---\n# stetdoc\n",
    )
    .unwrap()
}

#[test]
fn render_snapshots() {
    let root = Scratch::new("r22-snapshots");
    let paths = profile_paths(&root);
    let (profile, refused) = ClauseoProfile::new(paths.clone(), None).with_builtin_skills(None);
    assert!(refused.is_empty(), "{refused:?}");
    write_skills(&profile.skill_files(Some(AgentKind::Claude)).unwrap()).unwrap();
    insta::assert_snapshot!("claude", tree_text(paths.claude_plugin()));

    // Codex gets no skills folder: its index in `developer_instructions` points at Claude's.
    assert!(profile.skill_files(Some(AgentKind::Codex)).is_none());
    assert!(!paths.codex_skills().exists());
    let index = profile
        .skills_index()
        .replace(&root.to_string_lossy().into_owned(), "<root>")
        .replace('\\', "/");
    insta::assert_snapshot!("codex_index", index);
}

#[test]
fn no_shell_injection() {
    let root = Scratch::new("r22-injection");
    let paths = profile_paths(&root);
    let (profile, _) =
        ClauseoProfile::new(paths.clone(), None).with_builtin_skills(Some(stetdoc()));
    write_skills(&profile.skill_files(Some(AgentKind::Claude)).unwrap()).unwrap();
    let files = files_under(paths.claude_plugin());
    assert!(files.len() > BUILTIN.len());
    for (path, bytes) in files {
        assert!(
            !bytes
                .windows(SHELL_INJECTION.len())
                .any(|window| window == SHELL_INJECTION.as_bytes()),
            "{}",
            path.display()
        );
    }
}

#[test]
fn manifest_detects_change() {
    let root = Scratch::new("r22-drift");
    let lawyer = root.join("lawyer-skill").join(SKILL_MD);
    let system = root.join(".system").join("codex").join(SKILL_MD);
    write::fresh_dir(lawyer.parent().unwrap()).unwrap();
    write::fresh_dir(system.parent().unwrap()).unwrap();
    write::write(&lawyer, b"the lawyer's own").unwrap();
    write::write(&system, b"codex's own").unwrap();

    let rendered = render(&sources().unwrap()).unwrap();
    let first = write_rendered(&root, &rendered).unwrap();
    assert_eq!(first.drifted, Vec::<std::path::PathBuf>::new());
    let target = root.join("legal-research").join(SKILL_MD);
    let mut bytes = read(&target);
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    write::write(&target, &bytes).unwrap();

    let second = write_rendered(&root, &rendered).unwrap();
    assert_eq!(second.drifted, std::slice::from_ref(&target));
    assert_eq!(second.written, std::slice::from_ref(&target));
    let want = rendered.skills()[1].files()[0].bytes.clone();
    assert_eq!(read(&target), want);
    assert_eq!(read(&lawyer), b"the lawyer's own");
    assert_eq!(read(&system), b"codex's own");
    let entries = manifest(&root)["skills"].as_object().unwrap().clone();
    let names: Vec<&str> = entries.keys().map(String::as_str).collect();
    assert_eq!(names, BUILTIN);
}

#[test]
fn stetdoc_skill_untouched() {
    let root = Scratch::new("r22-stetdoc");
    let paths = profile_paths(&root);
    for folder in [paths.claude_skills(), paths.codex_skills().to_owned()] {
        write::fresh_dir(&folder).unwrap();
        write_rendered(&folder, &render(&[stetdoc()]).unwrap()).unwrap();
        let stetdoc_files = files_under(&folder.join(STETDOC));
        let stetdoc_entry = manifest(&folder)["skills"][STETDOC].clone();
        assert!(!stetdoc_files.is_empty() && stetdoc_entry.is_object());

        let rendered = render(&sources().unwrap()).unwrap();
        write_rendered(&folder, &rendered).unwrap();
        let drifted = folder.join("matter-brief").join(SKILL_MD);
        write::write(&drifted, b"changed").unwrap();
        // An old entry of R22's lists a file the skill no longer has: cleanup removes it.
        let stale = folder
            .join("legal-research")
            .join("references")
            .join("old.md");
        write::fresh_dir(stale.parent().unwrap()).unwrap();
        write::write(&stale, b"old").unwrap();
        let mut old = manifest(&folder);
        old["skills"]["legal-research"]["files"]["references/old.md"] = "0".repeat(64).into();
        write::write(
            &folder.join(MANIFEST),
            serde_json::to_string_pretty(&old).unwrap().as_bytes(),
        )
        .unwrap();
        let report = write_rendered(&folder, &rendered).unwrap();
        assert_eq!(report.drifted, [drifted]);
        assert_eq!(report.removed, std::slice::from_ref(&stale));
        assert!(!stale.exists());

        assert_eq!(files_under(&folder.join(STETDOC)), stetdoc_files);
        assert_eq!(manifest(&folder)["skills"][STETDOC], stetdoc_entry);
        assert!(folder.join(MANIFEST).exists());
    }
}

#[test]
fn a_retired_builtin_is_removed_and_nothing_else() {
    let root = Scratch::new("r22-retired");
    let old = SkillSource::verbatim(
        "old-skill",
        "1",
        vec![
            SkillFile {
                path: SKILL_MD.to_owned(),
                bytes: b"---\nname: old-skill\ndescription: \"d\"\n---\nold\n".to_vec(),
            },
            SkillFile {
                path: "references/a.md".to_owned(),
                bytes: b"a".to_vec(),
            },
        ],
    );
    let kept = SkillSource::verbatim(
        "kept-skill",
        "1",
        vec![SkillFile {
            path: SKILL_MD.to_owned(),
            bytes: b"---\nname: kept-skill\ndescription: \"d\"\n---\nkept\n".to_vec(),
        }],
    );
    write_rendered(&root, &render(&[stetdoc(), old.clone(), kept]).unwrap()).unwrap();
    // A folder with no manifest entry is not Clauseo's.
    let theirs = root.join("not-ours").join(SKILL_MD);
    write::fresh_dir(theirs.parent().unwrap()).unwrap();
    write::write(&theirs, b"the lawyer's").unwrap();
    let stetdoc_files = files_under(&root.join(STETDOC));
    let before = manifest(&root);

    let removed = retire(&root, &["old-skill", "not-ours"]).unwrap();
    assert_eq!(removed.len(), 2);
    assert!(!root.join("old-skill").exists());
    assert_eq!(read(&theirs), b"the lawyer's");
    assert_eq!(files_under(&root.join(STETDOC)), stetdoc_files);
    let after = manifest(&root);
    assert!(after["skills"].get("old-skill").is_none());
    assert_eq!(after["skills"][STETDOC], before["skills"][STETDOC]);
    assert_eq!(
        after["skills"]["kept-skill"],
        before["skills"]["kept-skill"]
    );

    // A file the lawyer put in a retired folder stays, with its folder.
    write_rendered(&root, &render(&[old]).unwrap()).unwrap();
    let extra = root.join("old-skill").join("mine.md");
    write::write(&extra, b"mine").unwrap();
    retire(&root, &["old-skill"]).unwrap();
    assert_eq!(read(&extra), b"mine");
    assert!(!root.join("old-skill").join(SKILL_MD).exists());
    assert!(!root.join("old-skill").join("references").exists());
    // Nothing to retire: nothing changes.
    assert_eq!(
        retire(&root, &RETIRED_BUILTINS).unwrap(),
        Vec::<std::path::PathBuf>::new()
    );
}

#[test]
fn a_codex_launch_writes_exactly_the_files_its_index_names() {
    let root = Scratch::new("r22-codex-launch");
    let paths = profile_paths(&root);
    let (profile, refused) =
        ClauseoProfile::new(paths.clone(), None).with_builtin_skills(Some(stetdoc()));
    assert!(refused.is_empty(), "{refused:?}");
    assert_eq!(profile.launch_writes(None), Vec::new());
    assert_eq!(profile.launch_writes(Some(AgentKind::Claude)).len(), 1);
    let writes = profile.launch_writes(Some(AgentKind::Codex));
    let done: Vec<_> = writes
        .iter()
        .map(|each| write_skills(each).unwrap())
        .collect();
    // The Claude plugin's write is first; its token puts the index in Codex's keys.
    let keys = profile.codex_keys(Some(AgentKind::Codex), &done[0]);
    assert_eq!(
        keys["developer_instructions"],
        format!("{}\n\n{}", profile.instructions(), profile.skills_index()).as_str()
    );
    assert!(
        profile
            .codex_keys(Some(AgentKind::Claude), &done[0])
            .is_empty()
    );
    // A token from another folder's write gives the instructions alone.
    let other = Scratch::new("r22-codex-other");
    let (elsewhere, _) =
        ClauseoProfile::new(profile_paths(&other), None).with_builtin_skills(Some(stetdoc()));
    let theirs = write_skills(&elsewhere.launch_writes(Some(AgentKind::Codex))[0]).unwrap();
    let keys = profile.codex_keys(Some(AgentKind::Codex), &theirs);
    assert_eq!(
        keys["developer_instructions"],
        profile.instructions().as_str()
    );
    let mut written: Vec<std::path::PathBuf> = writes
        .iter()
        .flat_map(|each| files_under(each.root()))
        .map(|(path, _)| path)
        .filter(|path| path.file_name().is_some_and(|name| name == SKILL_MD))
        .collect();
    written.sort();
    let mut listed: Vec<std::path::PathBuf> = profile
        .skills_index()
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|line| unspan(line.split_once("): ").unwrap().1).into())
        .collect();
    listed.sort();
    assert_eq!(listed, written);
    assert!(!paths.codex_skills().exists());
}

/// The text of a `CommonMark` code span.
fn unspan(span: &str) -> String {
    let fence = span.chars().take_while(|c| *c == '`').count();
    let inner = &span[fence..span.len() - fence];
    if inner.len() >= 2
        && inner.starts_with(' ')
        && inner.ends_with(' ')
        && !inner.trim().is_empty()
    {
        inner[1..inner.len() - 1].to_owned()
    } else {
        inner.to_owned()
    }
}

#[test]
fn every_index_path_is_where_the_write_puts_the_file() {
    let root = Scratch::new("r22 index `x` 'q'");
    let paths = profile_paths(&root);
    let (profile, refused) =
        ClauseoProfile::new(paths.clone(), None).with_builtin_skills(Some(stetdoc()));
    assert!(refused.is_empty(), "{refused:?}");
    write_skills(&profile.skill_files(Some(AgentKind::Claude)).unwrap()).unwrap();
    let index = profile.skills_index();
    let listed: Vec<String> = index
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|line| unspan(line.split_once("): ").unwrap().1))
        .collect();
    assert_eq!(listed.len(), BUILTIN.len() + 1);
    let rendered = render(
        &std::iter::once(stetdoc())
            .chain(sources().unwrap())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for (listed, skill) in listed.iter().zip(rendered.skills()) {
        let want = paths.claude_skills().join(skill.name()).join(SKILL_MD);
        assert_eq!(Path::new(listed), want);
        let md = skill.files().iter().find(|f| f.path == SKILL_MD).unwrap();
        assert_eq!(read(Path::new(listed)), md.bytes, "{listed}");
    }
}
