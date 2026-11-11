//! The folder maker's link rule and modes (S18 security review, drift: folders follow links).

use super::{TestDir, create_private_dirs_under};

/// A missing folder and its missing parents are made under the data folder; one already there
/// is fine.
#[test]
fn makes_missing_folders_under_the_data_folder() {
    let data = TestDir::new("dirs-make");
    let target = data.0.join("agents").join("codex-acp").join("home");
    create_private_dirs_under(&data.0, &target).unwrap();
    assert!(std::fs::symlink_metadata(&target).unwrap().is_dir());
    create_private_dirs_under(&data.0, &target).unwrap();
}

/// A link below the data folder is refused and never followed: nothing is made where it
/// points.
#[cfg(unix)]
#[test]
fn refuses_a_linked_component_below_the_data_folder() {
    let data = TestDir::new("dirs-link");
    let elsewhere = TestDir::new("dirs-link-target");
    std::fs::create_dir_all(data.0.join("agents")).unwrap();
    super::symlink(&elsewhere.0, &data.0.join("agents").join("codex-acp")).unwrap();
    let target = data.0.join("agents").join("codex-acp").join("home");
    let err = create_private_dirs_under(&data.0, &target).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{err}");
    assert!(!elsewhere.0.join("home").exists());
    // A link standing at the target itself is refused too.
    let at_target = data.0.join("agents").join("claude-acp");
    super::symlink(&elsewhere.0, &at_target).unwrap();
    assert!(create_private_dirs_under(&data.0, &at_target).is_err());
}

/// A file where a folder goes is refused, and a target outside the data folder too.
#[test]
fn refuses_a_file_in_the_way_and_a_target_outside() {
    let data = TestDir::new("dirs-file");
    data.file("agents", b"");
    assert!(create_private_dirs_under(&data.0, &data.0.join("agents").join("x")).is_err());
    let other = TestDir::new("dirs-outside");
    assert!(create_private_dirs_under(&data.0, &other.0.join("x")).is_err());
    assert!(!other.0.join("x").exists());
}

/// The data folder itself may be a link (`CLAUSEO_DATA_DIR` may name one).
#[cfg(unix)]
#[test]
fn follows_a_linked_data_folder() {
    let real = TestDir::new("dirs-real");
    let holder = TestDir::new("dirs-holder");
    let data = holder.0.join("data");
    super::symlink(&real.0, &data).unwrap();
    create_private_dirs_under(&data, &data.join("agents").join("signin")).unwrap();
    assert!(real.0.join("agents").join("signin").is_dir());
}

/// Each folder it makes is 0700 on Unix, whatever the umask.
#[cfg(unix)]
#[test]
fn new_folders_are_private() {
    use std::os::unix::fs::PermissionsExt as _;

    let data = TestDir::new("dirs-mode");
    let target = data.0.join("agents").join("codex-acp").join("home");
    create_private_dirs_under(&data.0, &target).unwrap();
    for folder in [
        data.0.join("agents"),
        data.0.join("agents").join("codex-acp"),
        target,
    ] {
        let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{}", folder.display());
    }
}

/// S18 re-review, Low: a real folder already there is set to 0700 too, as host's `make_home`
/// does; a link is refused, never chmodded through.
#[cfg(unix)]
#[test]
fn an_existing_folder_is_made_private() {
    use std::os::unix::fs::PermissionsExt as _;

    let data = TestDir::new("dirs-premade");
    let home = data.0.join("agents").join("codex-acp").join("home");
    std::fs::create_dir_all(&home).unwrap();
    for folder in [
        data.0.join("agents"),
        home.parent().unwrap().to_owned(),
        home.clone(),
    ] {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    create_private_dirs_under(&data.0, &home).unwrap();
    let mode = std::fs::metadata(&home).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let mode = std::fs::metadata(data.0.join("agents"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
}

/// A read-only tree (folders `0o555`, as Python's runtime is unpacked) goes with
/// `remove_dir_all`, made writable first; a link inside it is removed, never followed, and the
/// folder it points to keeps its mode.
#[cfg(unix)]
#[test]
fn a_read_only_tree_is_removed_without_following_a_link() {
    use std::os::unix::fs::PermissionsExt as _;

    use super::{make_read_only, remove_dir_all, seal_below, set_mode, symlink};
    let dir = TestDir::new("read-only-tree");
    let outside = dir.0.join("outside");
    dir.file("outside/kept", b"x");
    set_mode(&outside, 0o555).unwrap();
    let tree = dir.0.join("tree");
    dir.file("tree/a/b/file", b"y");
    symlink(&outside, &tree.join("a").join("link")).unwrap();
    seal_below(&tree).unwrap();
    make_read_only(&tree).unwrap();
    assert!(std::fs::remove_dir_all(&tree).is_err(), "std alone refuses");
    remove_dir_all(&tree).unwrap();
    assert!(!tree.exists());
    assert!(outside.join("kept").exists());
    let mode = std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o555);
    set_mode(&outside, 0o755).unwrap();
}

/// The lock files' rules, which two installs in one process rely on as much as two processes
/// do: every open is its own holder (`flock` on Unix, `LockFileEx` on Windows), so an exclusive
/// hold excludes a second open in this very process; shared holds go together; an exclusive and
/// a shared one never do; and a closed file gives its lock up. Each agent's lock is its own.
#[test]
fn agents_locks_exclude_each_other_within_one_process() {
    use super::{AgentsLock, Mode, try_lock};
    use clauseo_protocol::AgentId;

    let data = TestDir::new("locks");
    let a = AgentId::new("claude-acp").unwrap();
    let b = AgentId::new("codex-acp").unwrap();
    let take = |lock, mode| try_lock(&data.0, lock, mode).unwrap();

    let own_a = take(AgentsLock::Agent(&a), Mode::Exclusive).expect("A's lock is free");
    assert!(
        take(AgentsLock::Agent(&a), Mode::Exclusive).is_none(),
        "A's lock twice"
    );
    assert!(
        take(AgentsLock::Agent(&b), Mode::Exclusive).is_some(),
        "B's is its own"
    );
    drop(own_a);
    assert!(
        take(AgentsLock::Agent(&a), Mode::Exclusive).is_some(),
        "given up on close"
    );

    let one = take(AgentsLock::All, Mode::Shared).expect("shared");
    let two = take(AgentsLock::All, Mode::Shared).expect("shared beside shared");
    assert!(
        take(AgentsLock::All, Mode::Exclusive).is_none(),
        "exclusive beside shared"
    );
    drop((one, two));
    let whole = take(AgentsLock::All, Mode::Exclusive).expect("exclusive once none share");
    assert!(
        take(AgentsLock::All, Mode::Shared).is_none(),
        "shared beside exclusive"
    );
    drop(whole);

    let node = take(AgentsLock::Node, Mode::Exclusive).expect("Node's lock");
    assert!(take(AgentsLock::Node, Mode::Shared).is_none());
    assert!(
        take(AgentsLock::Python, Mode::Exclusive).is_some(),
        "Python's is apart"
    );
    drop(node);
    let names: Vec<String> = std::fs::read_dir(data.0.join("agents"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    for name in [
        ".lock",
        ".lock-claude-acp",
        ".lock-codex-acp",
        ".lock-node",
        ".lock-python",
    ] {
        assert!(names.iter().any(|n| n == name), "{name} in {names:?}");
    }
}

/// A link standing where a lock file goes is refused, never followed (Unix; Windows refuses one
/// found by `symlink_metadata` the same way, `open_no_follow`).
#[cfg(unix)]
#[test]
fn a_link_at_a_lock_file_is_refused() {
    use super::{AgentsLock, Mode, try_lock};

    let data = TestDir::new("locks-link");
    let elsewhere = TestDir::new("locks-link-target");
    let target = elsewhere.file("victim", b"kept");
    std::fs::create_dir_all(data.0.join("agents")).unwrap();
    super::symlink(&target, &data.0.join("agents").join(".lock-node")).unwrap();
    assert!(try_lock(&data.0, AgentsLock::Node, Mode::Exclusive).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"kept");
}
