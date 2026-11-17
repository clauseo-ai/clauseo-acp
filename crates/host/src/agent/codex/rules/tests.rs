use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::super::{make_home, prepare_launch};
use super::remove_rules;
use crate::write;

/// A fresh folder under the temp folder for `test`, removed first.
fn scratch(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("clauseo-codex-{test}-{}", std::process::id()));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

/// The home under `data`, as `Locations::agent_home` makes it.
fn home_in(data: &Path) -> PathBuf {
    data.join("agents").join("codex-acp").join("home")
}

/// A folder check that checks nothing off Windows and passes a folder the test made on it.
fn any_check() -> crate::agent::folder_support::FolderCheck {
    let id = MatterId::new(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0002,
    ))
    .unwrap();
    crate::agent::folder_support::FolderCheck {
        matter: Some(id),
        source: crate::agent::folder_support::CheckSource::Windows,
    }
}

/// A data folder, a matter folder and the agent's home made, under a fresh scratch root.
fn launch_root(test: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = scratch(test);
    let data = root.join("data");
    let matter = root.join("matter");
    write::create_dir_all(&matter).unwrap();
    write::create_dir_all(&data).unwrap();
    let home = home_in(&data);
    make_home(&data, &home).unwrap();
    (root, data, matter, home)
}

/// The launch removes a `rules` folder an earlier "Always" left (the prefix rule of the live
/// run, and a second file), with all it holds, and leaves the rest of the home alone.
#[test]
fn the_launch_removes_planted_rules() {
    let (root, data, matter, home) = launch_root("rules-planted");
    let rules = home.join("rules");
    write::create_dir_all(&rules.join("sub")).unwrap();
    write::write_file(
        &rules.join("default.rules"),
        b"prefix_rule(pattern=[\"touch\"], decision=\"allow\")\n",
    )
    .unwrap();
    write::write_file(&rules.join("sub/other.rules"), b"x\n").unwrap();
    write::write_file(&home.join("auth.json"), b"{}").unwrap();
    prepare_launch(&any_check(), &matter, &data, &home).unwrap();
    assert!(std::fs::symlink_metadata(&rules).is_err());
    assert!(home.join("auth.json").is_file());
    assert!(home.join("config.toml").is_file());
    // Again, with nothing there: fine.
    prepare_launch(&any_check(), &matter, &data, &home).unwrap();
    write::remove_dir_all(&root).unwrap();
}

/// A link named `rules` is removed, never followed: the folder it points to keeps its file.
/// A plain file named `rules` is removed too.
#[test]
fn the_launch_removes_a_linked_rules_or_a_file_without_following() {
    let (root, data, matter, home) = launch_root("rules-link");
    let elsewhere = root.join("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    write::write_file(&elsewhere.join("default.rules"), b"x\n").unwrap();
    let rules = home.join("rules");
    write::folder_link(&elsewhere, &rules).unwrap();
    prepare_launch(&any_check(), &matter, &data, &home).unwrap();
    assert!(std::fs::symlink_metadata(&rules).is_err());
    assert!(elsewhere.join("default.rules").is_file());

    write::write_file(
        &rules,
        b"prefix_rule(pattern=[\"touch\"], decision=\"allow\")\n",
    )
    .unwrap();
    remove_rules(&home).unwrap();
    assert!(std::fs::symlink_metadata(&rules).is_err());
    write::remove_dir_all(&root).unwrap();
}

/// A `rules` that cannot be removed (a folder in it the user may not change) is set aside, as
/// the launch's clearing does, and the launch goes on: Codex finds no `rules` to load.
#[cfg(unix)]
#[test]
fn rules_that_cannot_be_removed_are_set_aside_and_the_launch_goes_on() {
    let (root, data, matter, home) = launch_root("rules-stuck");
    let locked = home.join("rules").join("locked");
    write::create_dir_all(&locked).unwrap();
    write::write_file(&locked.join("default.rules"), b"x\n").unwrap();
    write::set_mode(&locked, 0o500).unwrap();
    // Root ignores the mode: then nothing here can fail, and the test has nothing to show.
    if write::write_file(&locked.join("probe"), b"").is_ok() {
        write::set_mode(&locked, 0o700).unwrap();
        write::remove_dir_all(&root).unwrap();
        return;
    }
    let launched = prepare_launch(&any_check(), &matter, &data, &home);
    let gone = std::fs::symlink_metadata(home.join("rules")).is_err();
    for entry in std::fs::read_dir(&home).unwrap() {
        let path = entry.unwrap().path();
        if path.join("locked").is_dir() {
            write::set_mode(&path.join("locked"), 0o700).unwrap();
        }
    }
    launched.unwrap();
    assert!(gone, "rules was set aside");
    assert!(home.join("config.toml").exists());
    write::remove_dir_all(&root).unwrap();
}
