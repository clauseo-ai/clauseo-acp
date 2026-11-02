//! R22's three built-in skills on a real disk: the render snapshots for Claude and Codex, the
//! drift check, `stetdoc` and every other folder left alone, and the Codex index's paths, each
//! exactly where the write puts the file.

use std::path::Path;

use clauseo_profile::skills::builtin::{BUILTIN, sources};
use clauseo_profile::skills::manifest::MANIFEST;
use clauseo_profile::skills::stetdoc::{STETDOC, skill_source};
use clauseo_profile::skills::{SHELL_INJECTION, SKILL_MD, render, write_rendered};
use clauseo_profile::test_support::tree_text;
use clauseo_profile::{ClauseoProfile, write_skills};
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
        let report = write_rendered(&folder, &rendered).unwrap();
        assert_eq!(report.drifted, [drifted]);

        assert_eq!(files_under(&folder.join(STETDOC)), stetdoc_files);
        assert_eq!(manifest(&folder)["skills"][STETDOC], stetdoc_entry);
        assert!(folder.join(MANIFEST).exists());
    }
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
