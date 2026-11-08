//! Nothing a window can reach (commands, the bridge, capabilities, permissions) names the
//! deletion: only the module itself and `jailed.rs`, which `main` reaches it through.

use std::path::{Path, PathBuf};

use super::sources::{manifest_dir, read};
use crate::delete_app_data::FLAG;

/// Every file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

/// Whether `line` names the module, its function or the stage's method (`delete_app_data` as a
/// whole word: a path, a call, a `use` with or without `as`), or the flag.
fn names_it(line: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    line.contains(FLAG)
        || line.match_indices("delete_app_data").any(|(at, name)| {
            let before = line[..at].chars().next_back();
            let after = line[at + name.len()..].chars().next();
            !before.is_some_and(word) && !after.is_some_and(word)
        })
}

#[test]
fn every_way_of_naming_the_deletion_is_seen() {
    for named in [
        "AppStages.delete_app_data(&args);",
        "use crate::{delete_app_data as d};",
        "use crate::{bridge, delete_app_data};",
        "crate::delete_app_data::run(args, &mut err)",
        "mod delete_app_data;",
        "\"--delete-app-data\"",
    ] {
        assert!(names_it(named), "{named}");
    }
    for unnamed in [
        "fn the_delete_app_data_box_says_clauseos_words() {",
        "deleteAppData",
    ] {
        assert!(!names_it(unnamed), "{unnamed}");
    }
}

#[test]
fn nothing_a_window_can_reach_names_the_deletion() {
    let dir = manifest_dir();
    // `main` reaches it through `launch` (jailed.rs) alone; lib.rs only declares the module.
    let allowed = |relative: &str| {
        relative == "src/delete_app_data.rs"
            || relative.starts_with("src/delete_app_data/")
            || relative == "src/jailed.rs"
            || relative == "src/jailed/tests.rs"
    };
    let mut looked = 0;
    for folder in ["src", "capabilities", "permissions"] {
        for path in files_under(&dir.join(folder)) {
            let relative = path
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            looked += 1;
            if allowed(&relative) {
                continue;
            }
            let lines: Vec<&str> = text.lines().filter(|line| names_it(line)).collect();
            if relative == "src/lib.rs" {
                assert_eq!(lines, ["mod delete_app_data;"], "{relative}");
            } else {
                assert!(lines.is_empty(), "{relative}: {lines:?}");
            }
        }
    }
    for file in ["build.rs", "tauri.conf.json"] {
        assert!(!read(file).lines().any(names_it), "{file}");
    }
    assert!(looked > 50, "only {looked} files read");
}
