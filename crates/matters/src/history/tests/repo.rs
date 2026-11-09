//! The git dir: create and open, commits, publish and the timeline.

use std::path::{Path, PathBuf};

use super::{file, git_id, new_store, replace_file, write_file};
use crate::Error;
use crate::history::{GixStore, HistoryStore, TIMELINE_REF, recorded_worktree};
use crate::stat::StatCache;
use crate::test_dir::TestDir;
use crate::write;

#[test]
fn create_writes_only_head_config_objects_and_refs() {
    let dir = TestDir::new("history-create");
    let git_dir = dir.path().join("root").join("matter").join("history.git");
    let worktree = dir.path().join("my \"folder\" a\\b");
    GixStore::create(&git_dir, &worktree).unwrap();

    let mut names: Vec<String> = std::fs::read_dir(&git_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["HEAD", "config", "objects", "refs"]);
    assert_eq!(
        std::fs::read_dir(git_dir.join("objects")).unwrap().count(),
        0
    );
    assert_eq!(std::fs::read_dir(git_dir.join("refs")).unwrap().count(), 0);
    assert_eq!(
        std::fs::read(git_dir.join("HEAD")).unwrap(),
        b"ref: refs/heads/clauseo\n"
    );
    let quoted = worktree
        .to_str()
        .unwrap()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    assert!(quoted.ends_with("my \\\"folder\\\" a\\\\b"), "{quoted}");
    let config = format!(
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = false\n\tbare = false\n\
         \tlogallrefupdates = false\n\tworktree = \"{quoted}\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(git_dir.join("config")).unwrap(),
        config
    );
}

#[test]
fn create_refuses_an_existing_history() {
    let dir = TestDir::new("history-create-twice");
    let (git_dir, _store) = new_store(&dir);
    let again = GixStore::create(&git_dir, &dir.path().join("folder"));
    assert!(
        matches!(again, Err(Error::HistoryExists { .. })),
        "{again:?}"
    );
}

#[test]
fn open_rewrites_the_worktree_after_a_move() {
    let dir = TestDir::new("history-open-moved");
    let (git_dir, store) = new_store(&dir);
    drop(store);
    let moved = dir.path().join("moved");
    GixStore::open(&git_dir, &moved).unwrap();
    let config = std::fs::read_to_string(git_dir.join("config")).unwrap();
    let quoted = moved.to_str().unwrap().replace('\\', "\\\\");
    assert!(
        config.ends_with(&format!("\tworktree = \"{quoted}\"\n")),
        "{config}"
    );
    assert!(!config.contains("folder\""), "{config}");
}

#[test]
fn open_rewrites_a_head_that_names_anything_else() {
    let dir = TestDir::new("history-open-head");
    let (git_dir, store) = new_store(&dir);
    drop(store);
    let head = git_dir.join("HEAD");
    replace_file(&head, b"ref: refs/clauseo/timeline\n");
    GixStore::open(&git_dir, &dir.path().join("folder")).unwrap();
    assert_eq!(std::fs::read(&head).unwrap(), b"ref: refs/heads/clauseo\n");
}

#[test]
fn open_does_not_follow_config_includes() {
    let dir = TestDir::new("history-open-include");
    let (git_dir, store) = new_store(&dir);
    drop(store);
    let config_path = git_dir.join("config");
    let mut config = std::fs::read(&config_path).unwrap();
    config.extend_from_slice(b"[include]\n\tpath = extra.config\n");
    replace_file(&config_path, &config);
    write_file(
        &git_dir.join("extra.config"),
        b"[core]\n\tworktree = /elsewhere\n",
    );

    // Were the include read, core.worktree would differ and open would rewrite the config.
    GixStore::open(&git_dir, &dir.path().join("folder")).unwrap();
    assert_eq!(std::fs::read(&config_path).unwrap(), config);
}

/// A `history.git` in `dir` whose own `config` is `config`, as a bare-style git dir.
fn git_dir_with_config(dir: &TestDir, config: &str) -> PathBuf {
    let git_dir = dir.path().join("history.git");
    write::create_dir_all(&git_dir.join("objects")).unwrap();
    write::create_dir_all(&git_dir.join("refs")).unwrap();
    write_file(&git_dir.join("HEAD"), b"ref: refs/heads/clauseo\n");
    write_file(&git_dir.join("config"), config.as_bytes());
    git_dir
}

/// Points git's configuration at files under `dir`: `HOME` at `home` (its `.gitconfig`), the
/// global file at `global`, the system file at `system`, and the system file switched on. These
/// are what gix reads when its permissions allow, so a repository that reads them returns their
/// `core.worktree`.
///
/// Edition 2024 makes `set_var` unsafe. Nothing else in the process reads or writes the
/// environment while it runs: nextest runs each test in its own process, and the harness's main
/// thread only waits for the test.
#[expect(
    unsafe_code,
    reason = "set_var is unsafe in edition 2024; nextest runs each test in its own process"
)]
fn point_git_config_at(home: &Path, global: &Path, system: &Path) {
    // SAFETY: see the function's doc: no other thread of this process reads the environment.
    unsafe {
        std::env::set_var("HOME", home);
        std::env::set_var("GIT_CONFIG_GLOBAL", global);
        std::env::set_var("GIT_CONFIG_SYSTEM", system);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "0");
    }
}

/// Writes `core.worktree = <value>` to the global, system and user config files under `dir`, and
/// points git at them.
fn point_every_config_at(dir: &TestDir, value: &str) {
    let home = dir.path().join("home");
    write::create_dir_all(&home).unwrap();
    let text = format!("[core]\n\tworktree = {value}\n");
    let global = dir.path().join("global.config");
    let system = dir.path().join("system.config");
    write_file(&global, text.as_bytes());
    write_file(&system, text.as_bytes());
    write_file(&home.join(".gitconfig"), text.as_bytes());
    point_git_config_at(&home, &global, &system);
}

#[test]
fn recorded_worktree_ignores_the_global_system_and_user_config() {
    // gix's isolated options read the repository's own config and none of these.
    let dir = TestDir::new("history-recorded-isolated");
    let git_dir = git_dir_with_config(&dir, "[core]\n\tbare = false\n");
    point_every_config_at(&dir, "/somewhere/else");
    assert_eq!(recorded_worktree(&git_dir).unwrap(), None);
}

#[test]
fn recorded_worktree_reads_the_repository_config_over_the_environment() {
    let dir = TestDir::new("history-recorded-repo");
    let git_dir = git_dir_with_config(&dir, "[core]\n\tworktree = /repo/folder\n");
    point_every_config_at(&dir, "/somewhere/else");
    assert_eq!(
        recorded_worktree(&git_dir).unwrap(),
        Some(PathBuf::from("/repo/folder"))
    );
}

#[test]
fn published_commits_make_the_timeline_oldest_first() {
    let dir = TestDir::new("history-timeline");
    let (git_dir, mut store) = new_store(&dir);
    assert_eq!(store.timeline().unwrap(), Vec::new());
    let mut cache = StatCache::default();

    let first_tree = store.write_tree(&[file("a.txt", 1)]).unwrap();
    let first = store
        .write_commit(
            first_tree,
            None,
            "First\n\nClauseo-Kind: x\n",
            1_700_000_000,
        )
        .unwrap();
    let body = format!(
        "tree {}\nauthor Clauseo <> 1700000000 +0000\ncommitter Clauseo <> 1700000000 +0000\n\n\
         First\n\nClauseo-Kind: x\n",
        first_tree.to_hex()
    );
    assert_eq!(first, git_id("commit", body.as_bytes()));
    store.publish(first, None, &mut cache).unwrap();
    assert_eq!(store.new_object_files(), Vec::<PathBuf>::new());
    assert!(cache.written_at.is_some());

    let second_tree = store.write_tree(&[file("a.txt", 2)]).unwrap();
    let second = store
        .write_commit(second_tree, Some(first), "Second", 1_700_000_100)
        .unwrap();
    store.publish(second, Some(first), &mut cache).unwrap();

    let ref_text = std::fs::read_to_string(git_dir.join(TIMELINE_REF)).unwrap();
    assert_eq!(ref_text, format!("{}\n", second.to_hex()));
    let timeline = store.timeline().unwrap();
    let seen: Vec<_> = timeline
        .iter()
        .map(|c| (c.id, c.tree, c.message.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            (first, first_tree, "First\n\nClauseo-Kind: x\n"),
            (second, second_tree, "Second")
        ]
    );
}

#[test]
fn publish_with_a_wrong_expected_moves_nothing() {
    let dir = TestDir::new("history-wrong-expected");
    let (git_dir, mut store) = new_store(&dir);
    let mut cache = StatCache::default();
    let tree = store.write_tree(&[]).unwrap();
    let first = store.write_commit(tree, None, "First", 1).unwrap();
    store.publish(first, None, &mut cache).unwrap();
    let second = store.write_commit(tree, Some(first), "Second", 2).unwrap();
    let ref_path = git_dir.join(TIMELINE_REF);
    let before = std::fs::read(&ref_path).unwrap();

    assert!(store.publish(second, None, &mut cache).is_err());
    assert!(store.publish(second, Some(second), &mut cache).is_err());
    assert_eq!(std::fs::read(&ref_path).unwrap(), before);
    assert_eq!(store.timeline().unwrap().len(), 1);
    let names: Vec<_> = std::fs::read_dir(ref_path.parent().unwrap())
        .unwrap()
        .collect();
    assert_eq!(names.len(), 1, "a temp or lock file was left: {names:?}");
}
