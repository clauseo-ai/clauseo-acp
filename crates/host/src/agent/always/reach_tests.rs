//! A kept rule's reach on disk (`reach.rs`).

#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::path::{Path, PathBuf};

#[cfg(unix)]
use super::Program;
use super::Reach;
#[cfg(unix)]
use crate::agent::always::table::At;
use crate::pipeline::tests::support::TempDir;

#[cfg(unix)]
fn search(dirs: &[&Path]) -> Option<OsString> {
    std::env::join_paths(dirs).ok()
}

#[cfg(unix)]
fn program(name: &str, search: Option<OsString>, bundled: &[&Path]) -> Program {
    Program {
        name: name.to_owned(),
        at: At::Bundled,
        search,
        bundled: bundled.iter().map(|dir| dir.to_path_buf()).collect(),
    }
}

#[cfg(unix)]
fn tool(dir: &Path, name: &str) -> PathBuf {
    crate::write::create_dir_all(dir).unwrap();
    let file = dir.join(name);
    crate::write::write_file(&file, b"#!/bin/sh\n").unwrap();
    file
}

#[test]
fn nothing_to_check_is_no_answer() {
    let dir = TempDir::new("reach_empty");
    assert!(Reach::default().is_empty());
    assert_eq!(Reach::default().check(dir.path(), false), None);
}

/// A command's working folder and path words must land inside the matter by real path; a name
/// the agent swapped for a link to another matter after the rule was kept is caught at use.
#[cfg(unix)]
#[test]
fn a_path_swapped_for_a_link_after_the_keep_is_caught() {
    let dir = TempDir::new("reach_link");
    let matter = dir.path().join("m");
    let other = dir.path().join("other");
    crate::write::create_dir_all(&matter).unwrap();
    crate::write::create_dir_all(&other).unwrap();
    crate::write::write_file(&matter.join("notes.txt"), b"x").unwrap();
    crate::write::write_file(&other.join("secret.txt"), b"x").unwrap();
    let reach = Reach {
        places: vec![matter.clone(), matter.join("notes.txt")],
        ..Reach::default()
    };
    assert_eq!(reach.check(&matter, false), Some(true));
    crate::write::remove_file(&matter.join("notes.txt")).unwrap();
    crate::write::symlink(&other.join("secret.txt"), &matter.join("notes.txt")).unwrap();
    assert_eq!(reach.check(&matter, false), Some(false));
    // A working folder outside, or a name an agent reads its own instructions from.
    for place in [other, matter.join("AGENTS.md"), matter.join(".codex/x")] {
        let reach = Reach {
            places: vec![place.clone()],
            ..Reach::default()
        };
        assert_eq!(
            reach.check(&matter, false),
            Some(false),
            "{}",
            place.display()
        );
    }
    // A file not made yet is judged by its folder (`cp a b`, `mkdir x`).
    let new = Reach {
        places: vec![matter.join("new.txt")],
        ..Reach::default()
    };
    assert_eq!(new.check(&matter, false), Some(true));
}

/// The program is the first file of its name on the agent's search path, by real path in a
/// folder its entry names and never in the matter; a relative entry first fails.
#[cfg(unix)]
#[test]
fn the_program_is_found_only_where_its_entry_says() {
    let dir = TempDir::new("reach_program");
    let matter = dir.path().join("m");
    crate::write::create_dir_all(&matter).unwrap();
    let ours = dir.path().join("ours");
    let elsewhere = dir.path().join("elsewhere");
    tool(&ours, "stetdoc");
    assert!(program("stetdoc", search(&[&ours]), &[&ours]).found(&matter, false));
    // Not on the path, no path, not one of the entry's folders.
    assert!(!program("stetdoc", search(&[&elsewhere]), &[&ours]).found(&matter, false));
    assert!(!program("stetdoc", None, &[&ours]).found(&matter, false));
    assert!(!program("stetdoc", search(&[&ours]), &[&elsewhere]).found(&matter, false));
    // A file of the same name earlier on the path shadows it.
    tool(&elsewhere, "stetdoc");
    assert!(!program("stetdoc", search(&[&elsewhere, &ours]), &[&ours]).found(&matter, false));
    // A relative entry (the working folder, which may be the matter) before it.
    let relative = std::env::join_paths([Path::new("."), ours.as_path()]).ok();
    assert!(!program("stetdoc", relative, &[&ours]).found(&matter, false));
    let empty = std::env::join_paths([Path::new(""), ours.as_path()]).ok();
    assert!(!program("stetdoc", empty, &[&ours]).found(&matter, false));
    // A folder in the matter is never the program's, even when named.
    let bin = matter.join("bin");
    tool(&bin, "stetdoc");
    assert!(!program("stetdoc", search(&[&bin]), &[&bin]).found(&matter, false));
    // A link in the right folder to a file in the matter resolves into the matter.
    let linked = dir.path().join("linked");
    crate::write::create_dir_all(&linked).unwrap();
    crate::write::symlink(&bin.join("stetdoc"), &linked.join("stetdoc")).unwrap();
    assert!(!program("stetdoc", search(&[&linked]), &[&linked]).found(&matter, false));
    // A cmdlet is part of the shell.
    let cmdlet = Program {
        at: At::Cmdlet,
        ..program("Get-Content", None, &[])
    };
    assert!(cmdlet.found(&matter, true));
}

/// A system program on the agent's real search path.
#[cfg(target_os = "macos")]
#[test]
fn a_system_program_is_found_in_the_system_folders() {
    let dir = TempDir::new("reach_system");
    let spec =
        crate::agent::always::table::find("wc", super::super::plain::Grammar::Posix).unwrap();
    let system = |search: &[&str]| Program {
        name: "wc".to_owned(),
        at: spec.at,
        search: std::env::join_paths(search).ok(),
        bundled: Vec::new(),
    };
    assert!(system(&["/usr/bin", "/bin"]).found(dir.path(), false));
    let local = dir.path().join("local");
    tool(&local, "wc");
    let first = local.to_string_lossy().into_owned();
    assert!(!system(&[&first, "/usr/bin"]).found(dir.path(), false));
}

/// codex-acp shows a run of `bash <name>` as `<name>`: a file of the program's name in Codex's
/// working folder, even a dangling link, means the rule does not apply.
#[cfg(unix)]
#[test]
fn a_file_named_like_the_program_beside_codex_is_caught() {
    let dir = TempDir::new("reach_absent");
    let matter = dir.path().join("m");
    crate::write::create_dir_all(&matter).unwrap();
    let reach = Reach {
        places: vec![matter.clone()],
        absent: Some(matter.join("ls")),
        ..Reach::default()
    };
    assert_eq!(reach.check(&matter, false), Some(true));
    crate::write::symlink(&dir.path().join("gone"), &matter.join("ls")).unwrap();
    assert_eq!(reach.check(&matter, false), Some(false));
    crate::write::remove_file(&matter.join("ls")).unwrap();
    crate::write::write_file(&matter.join("ls"), b"rm -rf ~\n").unwrap();
    assert_eq!(reach.check(&matter, false), Some(false));
}
