use std::path::{Path, PathBuf};

use super::{ASIDE, LOADED, clear_home};
use crate::write;

/// A fresh home under the temp folder, named for `test`.
fn fresh(test: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!(
        "clauseo-claude-clear-{test}-{}",
        std::process::id()
    ));
    write::remove_dir_all(&home).unwrap();
    write::create_dir_all(&home).unwrap();
    home
}

/// The names in `home`, sorted.
fn names(home: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// What Claude Code keeps in its home, each left as it was.
const KEPT: [&str; 5] = [
    ".claude.json",
    ".credentials.json",
    "settings.json",
    "plans/note.md",
    "projects/p/tool-results/x.txt",
];

/// Each loaded name goes, a folder with what it holds or a file, and nothing is left aside;
/// Claude Code's own state stays byte for byte; `skills`, `commands` and `agents` do not exist
/// afterwards, not even empty. A second clear finds nothing.
#[test]
fn a_launch_removes_what_claude_code_loads_from_its_home() {
    let home = fresh("loaded");
    for name in LOADED {
        let path = home.join(name);
        if name.contains('.') {
            write::write_file(&path, b"planted").unwrap();
        } else {
            write::create_dir_all(&path.join("x")).unwrap();
            write::write_file(&path.join("x").join("SKILL.md"), b"planted").unwrap();
        }
    }
    for kept in KEPT {
        let path = kept.split('/').fold(home.clone(), |at, part| at.join(part));
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, kept.as_bytes()).unwrap();
    }
    clear_home(&home).unwrap();
    assert_eq!(
        names(&home),
        [
            ".claude.json",
            ".credentials.json",
            "plans",
            "projects",
            "settings.json"
        ]
    );
    for kept in KEPT {
        let path = kept.split('/').fold(home.clone(), |at, part| at.join(part));
        assert_eq!(std::fs::read(&path).unwrap(), kept.as_bytes());
    }
    for watched in ["skills", "commands", "agents"] {
        assert!(
            std::fs::symlink_metadata(home.join(watched)).is_err(),
            "{watched}"
        );
    }
    clear_home(&home).unwrap();
    assert!(!names(&home).iter().any(|name| name.starts_with(ASIDE)));
    write::remove_dir_all(&home).unwrap();
}

/// A loaded name that is a link to a folder elsewhere goes itself; the folder it led to keeps
/// every file.
#[cfg(unix)]
#[test]
fn a_linked_loaded_name_goes_and_its_target_stays() {
    let home = fresh("linked");
    let elsewhere = fresh("linked-target");
    write::write_file(&elsewhere.join("SKILL.md"), b"kept").unwrap();
    write::symlink(&elsewhere, &home.join("skills")).unwrap();
    clear_home(&home).unwrap();
    assert!(std::fs::symlink_metadata(home.join("skills")).is_err());
    assert_eq!(std::fs::read(elsewhere.join("SKILL.md")).unwrap(), b"kept");
    write::remove_dir_all(&home).unwrap();
    write::remove_dir_all(&elsewhere).unwrap();
}

/// A loaded name that cannot be renamed (the home not writable) refuses the launch with
/// Clauseo's words for a home it could not prepare. Skipped where the folder's mode does not
/// stop the writes (a root account).
#[cfg(unix)]
#[test]
fn a_loaded_name_that_cannot_be_moved_refuses_the_launch() {
    let home = fresh("stuck");
    write::create_dir_all(&home.join("skills")).unwrap();
    write::set_mode(&home, 0o500).unwrap();
    if write::create_dir_all(&home.join("probe")).is_ok() {
        eprintln!("skipped: the folder's mode does not stop this account's writes");
    } else {
        let error = clear_home(&home).unwrap_err();
        assert_eq!(error.code(), clauseo_protocol::ErrorCode::AgentLaunchFailed);
    }
    write::set_mode(&home, 0o700).unwrap();
    write::remove_dir_all(&home).unwrap();
}
