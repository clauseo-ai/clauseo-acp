use std::path::{Path, PathBuf};

use clauseo_protocol::AgentKind;
use clauseo_settings::Locations;

use super::{Cleared, MARKER, clear, clear_once, codex_home};
use crate::write;

fn scratch(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "clauseo-codex-memories-{test}-{}",
        std::process::id()
    ));
    write::remove_dir_all(&root).unwrap();
    write::create_dir_all(&root).unwrap();
    root
}

/// What an earlier build may have left: both databases with their companions, and both
/// memory folders with a summary in each.
fn leave_memories(home: &Path) {
    write::create_dir_all(home).unwrap();
    for name in [
        "memories_1.sqlite",
        "memories_1.sqlite-wal",
        "memories_1.sqlite-shm",
        "memories_v2_1.sqlite",
        "memories_v2_1.sqlite-journal",
    ] {
        write::write_file(&home.join(name), b"rows").unwrap();
    }
    for folder in ["memories", "memories_v2"] {
        write::create_dir_all(&home.join(folder).join("rollout_summaries")).unwrap();
        write::write_file(&home.join(folder).join("memory_summary.md"), b"a fact").unwrap();
    }
}

/// The names Codex's other stores and Clauseo's own files go by: none is touched.
const KEPT: [&str; 5] = [
    "auth.json",
    "config.toml",
    "state_5.sqlite",
    "goals_1.sqlite",
    "history.jsonl",
];

#[test]
fn the_home_is_the_one_locations_names() {
    let root = scratch("home");
    let locations = Locations::under_root(&root);
    assert_eq!(
        codex_home(locations.data_dir()),
        locations.agent_home(AgentKind::Codex.id())
    );
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn clear_removes_every_memory_store_and_nothing_else() {
    let root = scratch("clear");
    let home = root.join("home");
    leave_memories(&home);
    for name in KEPT {
        write::write_file(&home.join(name), b"kept").unwrap();
    }
    assert_eq!(
        clear(&home),
        Cleared {
            removed: 7,
            failed: 0
        }
    );
    let mut left: Vec<String> = std::fs::read_dir(&home)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    let mut kept = KEPT.map(str::to_owned).to_vec();
    kept.sort();
    assert_eq!(left, kept);
    // Nothing left: nothing to do, nothing logged.
    assert_eq!(clear(&home), Cleared::default());
    write::remove_dir_all(&root).unwrap();
}

/// A link of a store's name is removed itself; what it points to stays.
#[cfg(unix)]
#[test]
fn clear_never_follows_a_link() {
    let root = scratch("link");
    let home = root.join("home");
    let elsewhere = root.join("elsewhere");
    write::create_dir_all(&home).unwrap();
    write::create_dir_all(&elsewhere).unwrap();
    write::write_file(&elsewhere.join("memory_summary.md"), b"the lawyer's").unwrap();
    write::symlink(&elsewhere, &home.join("memories")).unwrap();
    assert_eq!(clear(&home).removed, 1);
    assert!(std::fs::symlink_metadata(home.join("memories")).is_err());
    assert!(elsewhere.join("memory_summary.md").exists());
    write::remove_dir_all(&root).unwrap();
}

#[test]
fn clear_of_a_missing_home_does_nothing() {
    let root = scratch("missing");
    assert_eq!(clear(&root.join("no-home")), Cleared::default());
    write::remove_dir_all(&root).unwrap();
}

/// Only once per data folder: the store Codex makes again, empty, at its next start is left to it.
#[test]
fn clear_once_runs_once_per_data_folder() {
    let root = scratch("once");
    let data = root.join("data");
    let home = codex_home(&data);
    leave_memories(&home);
    assert_eq!(clear_once(&data).map(|cleared| cleared.removed), Some(7));
    assert!(home.parent().unwrap().join(MARKER).exists());
    write::write_file(&home.join("memories_1.sqlite"), b"fresh").unwrap();
    assert_eq!(clear_once(&data), None);
    assert!(home.join("memories_1.sqlite").exists());
    write::remove_dir_all(&root).unwrap();
}

/// No Codex home yet (no Codex ever ran): nothing to clear, and no folder made for the marker.
#[test]
fn clear_once_without_a_home_makes_nothing() {
    let root = scratch("fresh");
    let data = root.join("data");
    write::create_dir_all(&data).unwrap();
    assert_eq!(clear_once(&data), Some(Cleared::default()));
    assert!(!data.join("agents").exists());
    write::remove_dir_all(&root).unwrap();
}
