//! Tests of `tree.rs` on scratch folders: what a copy keeps (bytes, times, symlinks), what it
//! refuses, and the times `set_times_in` sets.

use super::*;
use crate::scratch::Scratch;
use std::time::Duration;

/// 2001-09-09T01:46:40Z and a second later: times no copy made today could have by chance.
fn old(offset: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000 + offset)
}

/// The access and modification times of `path`, not following a symlink.
fn times(path: &Path) -> (SystemTime, SystemTime) {
    let meta = fs::symlink_metadata(path).expect("the entry is there");
    (
        meta.accessed().expect("an access time"),
        meta.modified().expect("a modification time"),
    )
}

fn set_old(path: &Path, offset: u64) {
    let times = FileTimes::new()
        .set_accessed(old(offset))
        .set_modified(old(offset + 1));
    Scratch::set_times(path, times).expect("set the times");
}

/// `src/debug` with a file, a folder holding a file, and (on Unix) a file link and a folder link,
/// every file and folder with old times; and `src/TAG`.
fn source(dir: &Path) -> std::path::PathBuf {
    let debug = dir.join("src/debug");
    Scratch::create_dir_all(debug.join("deps")).expect("folders");
    Scratch::write(debug.join("app"), "binary").expect("a file");
    Scratch::write(debug.join("deps/app-1.d"), "dep info").expect("a file in a folder");
    Scratch::write(
        dir.join("src/TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55",
    )
    .expect("a lone file");
    #[cfg(unix)]
    {
        Scratch::symlink("deps/app-1.d", debug.join("link")).expect("a file link");
        Scratch::symlink("deps", debug.join("deps-link")).expect("a folder link");
    }
    set_old(&debug.join("app"), 10);
    set_old(&debug.join("deps/app-1.d"), 20);
    set_old(&debug.join("deps"), 30);
    set_old(&debug, 40);
    set_old(&dir.join("src/TAG"), 50);
    debug
}

#[test]
fn a_copy_keeps_the_bytes_and_times_of_every_file_and_folder() {
    let dir = Scratch::new("write-tree-copy");
    let debug = source(&dir);
    let target = dir.join("new/target");
    let files = [
        (debug.join("app"), target.join("debug/app")),
        (
            debug.join("deps/app-1.d"),
            target.join("debug/deps/app-1.d"),
        ),
        (dir.join("src/TAG"), target.join("TAG")),
    ];
    let folders = [
        (debug.join("deps"), target.join("debug/deps")),
        (debug.clone(), target.join("debug")),
    ];
    // Read before the copy: reading an original may move its access time (relatime on Linux).
    let before: Vec<_> = files
        .iter()
        .chain(&folders)
        .map(|(from, _)| times(from))
        .collect();
    copy_into(&debug, &target, "debug").expect("the tree is copied");
    copy_into(&dir.join("src/TAG"), &target, "TAG").expect("the file is copied");
    for ((from, to), times_before) in files.iter().chain(&folders).zip(before) {
        assert_eq!(times(to), times_before, "{}", to.display());
        if to.is_file() {
            assert_eq!(fs::read(to).expect("copied"), fs::read(from).expect("read"));
        } else {
            assert!(to.is_dir(), "{}", to.display());
        }
    }
    assert_eq!(
        times(&target.join("debug/app")).1,
        old(11),
        "an old time came along"
    );
}

#[cfg(unix)]
#[test]
fn a_copy_keeps_symlinks_as_symlinks_with_their_own_times_and_a_files_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new("write-tree-links");
    let debug = source(&dir);
    Scratch::set_permissions(debug.join("app"), fs::Permissions::from_mode(0o555))
        .expect("a read-only file");
    let target = dir.join("new/target");
    let links = [("link", "deps/app-1.d"), ("deps-link", "deps")];
    let before: Vec<_> = links
        .iter()
        .map(|(name, _)| times(&debug.join(name)))
        .collect();
    copy_into(&debug, &target, "debug").expect("the tree is copied");
    for ((name, pointed), times_before) in links.into_iter().zip(before) {
        let link = target.join("debug").join(name);
        assert!(link.is_symlink(), "{name} stays a symlink");
        assert_eq!(fs::read_link(&link).expect("a link"), Path::new(pointed));
        assert_eq!(times(&link), times_before, "{name}'s own times");
    }
    let copied = fs::metadata(target.join("debug/app")).expect("copied");
    assert_eq!(copied.permissions().mode() & 0o777, 0o555);
    assert_eq!(times(&target.join("debug/app")).1, old(11));
    Scratch::set_permissions(debug.join("app"), fs::Permissions::from_mode(0o644))
        .expect("writable again");
}

#[test]
fn a_copy_never_replaces_what_is_there_and_refuses_a_name_that_leaves_the_folder() {
    let dir = Scratch::new("write-tree-refused");
    let debug = source(&dir);
    let target = dir.join("new/target");
    Scratch::create_dir_all(target.join("debug")).expect("a folder in the way");
    Scratch::write(target.join("debug/kept"), "kept").expect("a file in it");
    let there = copy_into(&debug, &target, "debug").expect_err("refused");
    assert_eq!(there.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(
        there.to_string(),
        format!(
            "{} is already there, so nothing is copied over it",
            target.join("debug").display()
        )
    );
    assert_eq!(
        fs::read_dir(target.join("debug")).expect("read").count(),
        1,
        "nothing was copied into it"
    );
    for name in ["../x", "a/b", "/tmp/x", "", ".", "..", "a\\b"] {
        let refused = copy_into(&debug, &dir.join("other"), name).expect_err("refused");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(
            refused.to_string(),
            format!("`{name}` is not a name in the folder: it must be one plain file name")
        );
    }
    let missing = copy_into(&dir.join("src/none"), &dir.join("other"), "none");
    assert_eq!(
        missing.expect_err("refused").kind(),
        io::ErrorKind::NotFound
    );
    assert!(!dir.join("other").exists(), "a refused copy makes nothing");
}

#[test]
fn set_times_in_sets_both_times_of_a_path_under_the_folder() {
    let dir = Scratch::new("write-tree-times");
    Scratch::create_dir_all(dir.join("a/b")).expect("folders");
    Scratch::write(dir.join("a/b/c.rs"), "x").expect("a file");
    set_times_in(&dir, "a/b/c.rs", old(0)).expect("set");
    assert_eq!(times(&dir.join("a/b/c.rs")), (old(0), old(0)));
    for path in [
        "../x", "/tmp/x", "a//c.rs", "./a", "a/../b", "", "a/", "a\\b",
    ] {
        let refused = set_times_in(&dir, path, old(0)).expect_err("refused");
        assert_eq!(refused.kind(), io::ErrorKind::InvalidInput, "{path}");
        assert_eq!(
            refused.to_string(),
            format!("`{path}` is not a relative path of plain names joined by /")
        );
    }
    let missing = set_times_in(&dir, "a/none", old(0)).expect_err("no such file");
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);
}

#[cfg(unix)]
#[test]
fn set_times_in_sets_a_symlinks_own_times_and_leaves_its_target_alone() {
    let dir = Scratch::new("write-tree-times-link");
    Scratch::write(dir.join("target.rs"), "x").expect("a file");
    Scratch::symlink("target.rs", dir.join("link")).expect("a link");
    set_old(&dir.join("target.rs"), 100);
    set_times_in(&dir, "link", old(0)).expect("set");
    assert_eq!(times(&dir.join("link")), (old(0), old(0)));
    assert_eq!(times(&dir.join("target.rs")), (old(100), old(101)));
}
