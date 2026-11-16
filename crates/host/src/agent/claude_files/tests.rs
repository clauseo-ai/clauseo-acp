//! The Claude refusal list ([`super::refuse_claude_entries`]): each entry, in any form, refuses
//! the matter folder with its own code; a folder without one passes; a stat that fails fails
//! closed.

use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;

use super::{CLAUDE_REFUSED_ENTRIES, entry_path, refuse_claude_entries};
use crate::write;

/// A fresh folder under the temp folder for `test`, removed first.
fn scratch(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("clauseo-claude-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

/// The list, written out: a new entry changes this test, and its code is its own.
#[test]
fn the_refused_entries_are_listed_with_their_codes() {
    let rows: Vec<(&[&str], ErrorCode)> = CLAUDE_REFUSED_ENTRIES
        .iter()
        .map(|entry| (entry.path, entry.code))
        .collect();
    let tasks: &[&str] = &[".claude", "scheduled_tasks.json"];
    let git: &[&str] = &[".git"];
    assert_eq!(
        rows,
        [
            (tasks, ErrorCode::MatterClaudeTasksFile),
            (git, ErrorCode::MatterGitFolder),
        ]
    );
}

/// An entry's names are joined one by one: under a verbatim folder a `/` would be part of a
/// name, not a separator.
#[test]
fn an_entrys_names_are_joined_one_by_one() {
    let names = [".claude", "scheduled_tasks.json"];
    if cfg!(windows) {
        let joined = entry_path(Path::new(r"\\?\C:\Matters\Acme"), &names);
        assert_eq!(
            joined.to_str(),
            Some(r"\\?\C:\Matters\Acme\.claude\scheduled_tasks.json")
        );
    } else {
        let joined = entry_path(Path::new("/m/Acme"), &names);
        assert_eq!(
            joined.to_str(),
            Some("/m/Acme/.claude/scheduled_tasks.json")
        );
    }
    let relative = entry_path(Path::new("m"), &names);
    assert_eq!(relative.components().count(), 3);
}

/// Under a stored verbatim folder the refusal still finds the entry, and a clean folder passes.
#[cfg(windows)]
#[test]
fn a_verbatim_folder_is_checked_as_any_other() {
    let root = scratch("verbatim");
    let verbatim = PathBuf::from(format!(
        r"\\?\{}",
        dunce::canonicalize(&root).unwrap().display()
    ));
    assert!(refuse_claude_entries(&verbatim).is_ok());
    write::create_dir_all(&root.join(".claude")).unwrap();
    write::write_file(&root.join(".claude").join("scheduled_tasks.json"), b"{}").unwrap();
    let error = refuse_claude_entries(&verbatim).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterClaudeTasksFile);
    write::remove_dir_all(&root).unwrap();
}

/// `.claude/scheduled_tasks.json` in the matter folder, a file, a folder or an empty file,
/// refuses the folder; a `.claude` holding other files, or a `.claude` file, does not.
#[test]
fn a_matters_own_claude_task_file_of_any_kind_refuses_the_folder() {
    let root = scratch("refuse");
    let clean = root.join("clean");
    write::create_dir_all(&clean.join(".claude")).unwrap();
    write::write_file(&clean.join(".claude/settings.json"), b"{}").unwrap();
    assert!(refuse_claude_entries(&clean).is_ok());
    let dot_file = root.join("dot-file");
    write::create_dir_all(&dot_file).unwrap();
    write::write_file(&dot_file.join(".claude"), b"").unwrap();
    assert!(refuse_claude_entries(&dot_file).is_ok());

    for (name, folder) in [("file", false), ("empty-file", false), ("folder", true)] {
        let matter = root.join(name);
        let entry = matter.join(".claude/scheduled_tasks.json");
        if folder {
            write::create_dir_all(&entry).unwrap();
        } else {
            write::create_dir_all(&matter.join(".claude")).unwrap();
            let bytes: &[u8] = if name == "file" {
                b"{\"tasks\":[]}"
            } else {
                b""
            };
            write::write_file(&entry, bytes).unwrap();
        }
        let error = refuse_claude_entries(&matter).unwrap_err();
        assert_eq!(error.code(), ErrorCode::MatterClaudeTasksFile, "{name}");
        assert!(error.to_string().contains("rename or remove"), "{error}");
        // No path in the words the lawyer may see.
        assert!(
            !error.to_string().contains(&*root.to_string_lossy()),
            "{error}"
        );
    }
    write::remove_dir_all(&root).unwrap();
}

/// A link at the entry's name counts wherever it points, a dangling one too; so does a
/// `.claude` that is a link to a folder holding the file.
#[cfg(unix)]
#[test]
fn a_link_at_the_entry_counts_wherever_it_points() {
    let root = scratch("links");
    let dangling = root.join("dangling");
    write::create_dir_all(&dangling.join(".claude")).unwrap();
    write::symlink(
        &root.join("nowhere"),
        &dangling.join(".claude/scheduled_tasks.json"),
    )
    .unwrap();
    let error = refuse_claude_entries(&dangling).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterClaudeTasksFile);

    let elsewhere = root.join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    write::write_file(&elsewhere.join("scheduled_tasks.json"), b"{}").unwrap();
    let linked = root.join("linked");
    write::create_dir_all(&linked).unwrap();
    write::symlink(&elsewhere, &linked.join(".claude")).unwrap();
    let error = refuse_claude_entries(&linked).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterClaudeTasksFile);
    write::remove_dir_all(&root).unwrap();
}

/// A stat that fails for another reason than not-found (here a `.claude` the user may not
/// search) fails closed as `matter.folder_missing`. Skipped as root, who may search it anyway.
#[cfg(unix)]
#[test]
fn a_stat_that_fails_fails_closed_as_folder_missing() {
    let root = scratch("locked");
    let locked = root.join(".claude");
    write::create_dir_all(&locked).unwrap();
    write::set_mode(&locked, 0o000).unwrap();
    let as_root = std::fs::read_dir(&locked).is_ok();
    let refused = refuse_claude_entries(&root);
    write::set_mode(&locked, 0o700).unwrap();
    write::remove_dir_all(&root).unwrap();
    if as_root {
        return;
    }
    assert_eq!(refused.unwrap_err().code(), ErrorCode::MatterFolderMissing);
}

/// A matter folder that is gone, or is a file, is `matter.folder_missing`, not a pass.
#[test]
fn a_folder_that_is_gone_is_not_a_pass() {
    let root = scratch("gone");
    let gone = root.join("gone");
    let error = refuse_claude_entries(&gone).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterFolderMissing);
    let file = root.join("file");
    write::write_file(&file, b"").unwrap();
    let error = refuse_claude_entries(&file).unwrap_err();
    assert_eq!(error.code(), ErrorCode::MatterFolderMissing);
    write::remove_dir_all(&root).unwrap();
}

/// A `.git` in the matter folder itself, a folder, a file or a link (a dangling one too),
/// refuses the folder with its own code; a `.git` deeper in it, or in the folder above it, is
/// not this check's.
#[test]
fn a_git_entry_in_the_matter_folder_refuses_it() {
    let root = scratch("git");
    for (name, kind) in [("folder", 0), ("file", 1), ("link", 2)] {
        let matter = root.join(name);
        write::create_dir_all(&matter).unwrap();
        match kind {
            0 => write::create_dir_all(&matter.join(".git/objects")).unwrap(),
            1 => write::write_file(&matter.join(".git"), b"gitdir: /elsewhere\n").unwrap(),
            _ => {
                #[cfg(unix)]
                write::symlink(&root.join("nowhere"), &matter.join(".git")).unwrap();
                #[cfg(not(unix))]
                continue;
            }
        }
        let error = refuse_claude_entries(&matter).unwrap_err();
        assert_eq!(error.code(), ErrorCode::MatterGitFolder, "{name}");
        assert!(
            !error.to_string().contains(&*root.to_string_lossy()),
            "{error}"
        );
    }
    let above = root.join("above");
    write::create_dir_all(&above.join(".git")).unwrap();
    let inner = above.join("matter");
    write::create_dir_all(&inner.join("Drafts/.git")).unwrap();
    assert!(refuse_claude_entries(&inner).is_ok());
    write::remove_dir_all(&root).unwrap();
}
