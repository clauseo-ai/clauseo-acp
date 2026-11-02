use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use super::{Found, FoundKind, walk, walk_first};
use crate::PlaceholderProbe;
use crate::no_download::NoDownload;
use crate::test_dir::TestDir;
use crate::write;

/// Marks as cloud-only every path whose file name is in its list.
struct NamedProbe(Vec<&'static str>);

impl PlaceholderProbe for NamedProbe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| self.0.contains(&name))
    }
}

fn put(root: &Path, relative: &str, bytes: usize) {
    let path = root.join(relative);
    write::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = write::create_new(&path).unwrap();
    file.write_all(&vec![b'x'; bytes]).unwrap();
}

#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "the walk's test makes a symlink to check it is skipped"
)]
fn symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

fn paths(found: &[Found]) -> Vec<&str> {
    found.iter().map(|found| found.path.as_str()).collect()
}

fn walk_all(root: &Path, probe: &dyn PlaceholderProbe, cap: u64) -> Vec<Found> {
    walk(root, &NoDownload::begin(probe), cap, None, &mut |_| {})
        .unwrap()
        .unwrap()
        .files
}

#[test]
fn walk_sorts_by_bytes_and_skips_excluded_names() {
    let dir = TestDir::new("walk-sorts");
    let root = dir.path();
    put(root, "b.docx", 1);
    put(root, "a/z.txt", 2);
    put(root, "a.txt", 3);
    // Uppercase sorts before lowercase by bytes.
    put(root, "Z.docx", 4);
    put(root, "~$b.docx", 1);
    put(root, "x.tmp", 1);
    put(root, ".DS_Store", 1);
    put(root, ".git/HEAD", 1);
    put(root, ".git/objects/x", 1);
    put(root, ".clauseo/matter.json", 1);
    put(root, "a/.git", 1);
    put(root, "dir.tmp/kept.txt", 1);
    let found = walk_all(root, &NamedProbe(vec![]), u64::MAX);
    assert_eq!(
        paths(&found),
        ["Z.docx", "a.txt", "a/z.txt", "b.docx", "dir.tmp/kept.txt"]
    );
    assert!(found.iter().all(|found| found.kind == FoundKind::File));
    assert_eq!(found[1].len, 3);
}

#[test]
fn walk_marks_cloud_only_before_over_cap() {
    let dir = TestDir::new("walk-kinds");
    let root = dir.path();
    put(root, "small", 4);
    put(root, "exact", 5);
    put(root, "big", 6);
    put(root, "cloud", 6);
    let found = walk_all(root, &NamedProbe(vec!["cloud"]), 5);
    let kinds: Vec<(&str, FoundKind)> = found
        .iter()
        .map(|found| (found.path.as_str(), found.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("big", FoundKind::OverCap),
            ("cloud", FoundKind::CloudOnly),
            ("exact", FoundKind::File),
            ("small", FoundKind::File),
        ]
    );
}

#[cfg(unix)]
#[test]
fn walk_skips_symlinks_to_files_and_folders() {
    let dir = TestDir::new("walk-symlinks");
    let root = dir.path().join("matter");
    let outside = dir.path().join("outside");
    put(&outside, "secret.txt", 1);
    put(&root, "kept.txt", 1);
    symlink(&outside, &root.join("linked-folder"));
    symlink(&outside.join("secret.txt"), &root.join("linked-file.txt"));
    let found = walk_all(&root, &NamedProbe(vec![]), u64::MAX);
    assert_eq!(paths(&found), ["kept.txt"]);
}

#[test]
fn walk_calls_progress_once_per_file() {
    let dir = TestDir::new("walk-progress");
    put(dir.path(), "a", 1);
    put(dir.path(), "d/b", 1);
    let seen = Mutex::new(Vec::new());
    let found = walk(
        dir.path(),
        &NoDownload::begin(&NamedProbe(vec![])),
        u64::MAX,
        None,
        &mut |found| {
            seen.lock().unwrap().push(found.path.as_str().to_owned());
        },
    )
    .unwrap()
    .unwrap();
    let mut seen = seen.into_inner().unwrap();
    seen.sort();
    assert_eq!(seen, paths(&found.files));
}

#[test]
fn walk_stops_when_cancelled() {
    let dir = TestDir::new("walk-cancel");
    put(dir.path(), "a", 1);
    let cancel = AtomicBool::new(true);
    let walked = walk(
        dir.path(),
        &NoDownload::begin(&NamedProbe(vec![])),
        u64::MAX,
        Some(&cancel),
        &mut |_| {},
    );
    assert!(walked.unwrap().is_none());
}

#[test]
fn walk_stops_between_the_stats_of_one_folder() {
    let dir = TestDir::new("walk-cancel-stats");
    for name in ["a", "b", "c", "d"] {
        put(dir.path(), name, 1);
    }
    let cancel = AtomicBool::new(false);
    let mut stats = 0;
    let walked = walk(
        dir.path(),
        &NoDownload::begin(&NamedProbe(vec![])),
        u64::MAX,
        Some(&cancel),
        &mut |_| {
            stats += 1;
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        },
    );
    assert!(walked.unwrap().is_none());
    assert_eq!(stats, 1, "the walk went on taking stats after the cancel");
}

#[test]
fn walk_of_a_missing_folder_fails() {
    let dir = TestDir::new("walk-missing");
    let err = walk(
        &dir.path().join("gone"),
        &NoDownload::begin(&NamedProbe(vec![])),
        1,
        None,
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            crate::Error::Io {
                op: "read folder",
                ..
            }
        ),
        "{err:?}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn walk_skips_names_that_are_not_utf8() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let dir = TestDir::new("walk-not-utf8");
    put(dir.path(), "kept", 1);
    let bad = dir.path().join(OsStr::from_bytes(b"bad\xff"));
    write::create_dir_all(&bad).unwrap();
    put(&bad, "inner", 1);
    let found = walk_all(dir.path(), &NamedProbe(vec![]), u64::MAX);
    assert_eq!(paths(&found), ["kept"]);
}

/// Sets `path`'s mode while alive and puts back `0o755` when dropped.
#[cfg(unix)]
struct Mode(std::path::PathBuf);

#[cfg(unix)]
impl Mode {
    fn set(path: &Path, mode: u32) -> Self {
        use std::os::unix::fs::PermissionsExt;
        write::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        Self(path.to_path_buf())
    }
}

#[cfg(unix)]
impl Drop for Mode {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        write::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// A subfolder that lists but cannot be entered (mode `0o444`: its entries' stats are denied)
/// gives the same listing as one that does not list at all (`0o000`): unread, with nothing
/// under it found, and the rest of the matter walked.
#[cfg(unix)]
#[test]
fn walk_keeps_a_folder_that_lists_but_cannot_be_entered_as_unread() {
    let dir = TestDir::new("walk-unenterable");
    let root = dir.path();
    put(root, "a.txt", 1);
    put(root, "sub/b.txt", 1);
    put(root, "sub/deeper/c.txt", 1);
    put(root, "z/d.txt", 1);
    let listing = |mode| {
        let _mode = Mode::set(&root.join("sub"), mode);
        walk(
            root,
            &NoDownload::begin(&NamedProbe(vec![])),
            u64::MAX,
            None,
            &mut |_| {},
        )
        .unwrap()
        .unwrap()
    };
    let unlisted = listing(0o000);
    let unentered = listing(0o444);
    assert_eq!(paths(&unentered.files), ["a.txt", "z/d.txt"]);
    assert_eq!(unentered.unread, ["sub"]);
    assert_eq!(unentered, unlisted);
}

/// The root listing but not entering is an error, as the root not listing is: an unread root
/// would leave every file out.
#[cfg(unix)]
#[test]
fn walk_of_a_root_that_cannot_be_entered_fails() {
    let dir = TestDir::new("walk-unenterable-root");
    put(dir.path(), "matter/a.txt", 1);
    let root = dir.path().join("matter");
    let _mode = Mode::set(&root, 0o444);
    let err = walk(
        &root,
        &NoDownload::begin(&NamedProbe(vec![])),
        u64::MAX,
        None,
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(err, crate::Error::Io { op: "stat", .. }),
        "{err:?}"
    );
}

#[test]
fn walk_first_lists_what_walk_lists_in_order_and_stops_past_the_limit() {
    let dir = TestDir::new("walk-first");
    let root = dir.path();
    // `a.txt` sorts before `a/` and `a0` after it; uppercase before lowercase.
    for relative in [
        "a.txt",
        "a/b.txt",
        "a/a/c.txt",
        "a0",
        "a-b/x",
        "Z",
        "b/~$lock.docx",
        "b/c/d/e.txt",
        "c.tmp",
        ".git/HEAD",
        "z",
    ] {
        put(root, relative, 1);
    }
    let probe = NamedProbe(vec![]);
    let all = walk_all(root, &probe, u64::MAX);
    assert_eq!(
        paths(&all),
        [
            "Z",
            "a-b/x",
            "a.txt",
            "a/a/c.txt",
            "a/b.txt",
            "a0",
            "b/c/d/e.txt",
            "z"
        ]
    );
    for limit in 0..=all.len() + 1 {
        let first = walk_first(root, &NoDownload::begin(&probe), u64::MAX, limit).unwrap();
        let expected = &all[..all.len().min(limit + 1)];
        assert_eq!(first.files, expected, "limit {limit}");
    }
}

/// `walk_first` keeps a folder that lists but cannot be entered as unread, as `walk` does,
/// even when the cut falls where its files would be; the root not entering is an error.
#[cfg(unix)]
#[test]
fn walk_first_keeps_a_folder_that_cannot_be_entered_as_unread() {
    let dir = TestDir::new("walk-first-unenterable");
    let root = dir.path();
    put(root, "a.txt", 1);
    put(root, "sub/b.txt", 1);
    put(root, "sub/deeper/c.txt", 1);
    put(root, "z/d.txt", 1);
    let probe = NamedProbe(vec![]);
    let first = |limit| walk_first(root, &NoDownload::begin(&probe), u64::MAX, limit).unwrap();
    {
        let _mode = Mode::set(&root.join("sub"), 0o444);
        let all = first(100);
        assert_eq!(paths(&all.files), ["a.txt", "z/d.txt"]);
        assert_eq!(all.unread, ["sub"]);
        let walked = walk(
            root,
            &NoDownload::begin(&probe),
            u64::MAX,
            None,
            &mut |_| {},
        )
        .unwrap()
        .unwrap();
        assert_eq!(all, walked);
        // The cut falls inside `sub`: still nothing of it listed, the folder the entry past.
        let cut = first(1);
        assert_eq!(paths(&cut.files), ["a.txt"]);
        assert_eq!(cut.unread, ["sub"]);
    }
    let _mode = Mode::set(root, 0o444);
    let err = walk_first(root, &NoDownload::begin(&probe), u64::MAX, 100).unwrap_err();
    assert!(
        matches!(err, crate::Error::Io { op: "stat", .. }),
        "{err:?}"
    );
}
