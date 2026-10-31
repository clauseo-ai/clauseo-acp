use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use super::{Found, FoundKind, walk};
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
