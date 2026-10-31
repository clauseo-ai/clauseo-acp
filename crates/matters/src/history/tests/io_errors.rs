//! I/O failures in the git dir and in the files read: each is an error naming what failed,
//! never taken for a missing file, an empty timeline or a file that changed.

use std::io;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

use super::new_store;
#[cfg(unix)]
use super::{files_under, git_id, noise, write_file};
use crate::Error;
use crate::history::{GixStore, HistoryStore, TIMELINE_REF};
#[cfg(unix)]
use crate::no_download::NoDownload;
use crate::test_dir::TestDir;
use crate::write;
#[cfg(unix)]
use crate::{OsProbe, PlaceholderProbe};

/// A guard for a test's direct reads, with the OS probe.
#[cfg(unix)]
fn no_download() -> NoDownload<'static> {
    const PROBE: &dyn PlaceholderProbe = &OsProbe;
    NoDownload::begin(PROBE)
}

/// The `op` and kind of an [`Error::Io`], or `None` for any other result.
fn io_kind<T>(result: &Result<T, Error>) -> Option<(&'static str, io::ErrorKind)> {
    match result {
        Err(Error::Io { op, source, .. }) => Some((op, source.kind())),
        _ => None,
    }
}

/// A timeline ref that exists but does not read (here a folder) is an error, not a timeline
/// that does not exist yet: the next checkpoint must not start a new timeline over it.
#[test]
fn a_timeline_ref_that_does_not_read_is_an_error_not_an_empty_timeline() {
    let dir = TestDir::new("history-ref-unreadable");
    let (git_dir, store) = new_store(&dir);
    drop(store);
    write::create_dir_all(&git_dir.join(TIMELINE_REF)).unwrap();
    let store = GixStore::open(&git_dir, &dir.path().join("folder")).unwrap();

    let timeline = store.timeline();
    assert_eq!(
        io_kind(&timeline).map(|(op, _)| op),
        Some("read"),
        "{timeline:?}"
    );
}

/// A timeline ref that cannot be stat'd (its folder is a file) fails the open, rather than
/// opening as a history never published.
#[cfg(unix)]
#[test]
fn a_timeline_ref_that_cannot_be_stat_fails_the_open() {
    let dir = TestDir::new("history-ref-no-stat");
    let (git_dir, store) = new_store(&dir);
    drop(store);
    let folder = git_dir.join(TIMELINE_REF);
    write_file(folder.parent().unwrap(), b"not a folder");

    let opened = GixStore::open(&git_dir, &dir.path().join("folder"));
    assert_eq!(
        io_kind(&opened),
        Some(("stat", io::ErrorKind::NotADirectory)),
        "{opened:?}"
    );
}

/// A history folder that cannot be made for a reason other than its existence (a name too long
/// for the file system) is an I/O error, not "the history folder already exists".
#[test]
fn a_history_that_cannot_be_made_is_an_io_error_not_an_existing_history() {
    let dir = TestDir::new("history-create-too-long");
    let git_dir = dir.path().join("root").join("x".repeat(300));
    let created = GixStore::create(&git_dir, &dir.path().join("folder"));
    assert_eq!(
        io_kind(&created).map(|(op, _)| op),
        Some("create"),
        "{created:?}"
    );
}

/// An object whose file cannot be stat'd (its `objects/xx` folder is a file) is an I/O error
/// naming the object, not an absent object to write over.
#[cfg(unix)]
#[test]
fn a_present_object_that_cannot_be_stat_is_an_io_error() {
    let dir = TestDir::new("history-object-no-stat");
    let (git_dir, mut store) = new_store(&dir);
    let content = b"the signed lease\n";
    let hex = git_id("blob", content).to_hex();
    let (folder, name) = hex.split_at(2);
    write_file(&git_dir.join("objects").join(folder), b"not a folder");
    let path = dir.path().join("lease.txt");
    write_file(&path, content);

    let stored = store.store_file(&path, 17, u64::MAX, &no_download());
    let object = git_dir.join("objects").join(folder).join(name);
    assert!(
        matches!(
            &stored,
            Err(Error::Io { op: "stat", path, source })
                if *path == object && source.kind() == io::ErrorKind::NotADirectory
        ),
        "{stored:?}"
    );
}

/// A streamed file whose object cannot be written (a folder stands where its object file goes)
/// is a history error from the write: not a file that is not downloaded, and not a file that
/// kept changing.
#[cfg(unix)]
#[test]
fn a_streamed_file_whose_object_cannot_be_written_is_a_write_error() {
    let dir = TestDir::new("history-stream-unwritable");
    let (git_dir, mut store) = new_store(&dir);
    store.set_stream_threshold(1024);
    let content = noise(5000);
    let hex = git_id("blob", &content).to_hex();
    let (folder, name) = hex.split_at(2);
    write::create_dir_all(
        &git_dir
            .join("objects")
            .join(folder)
            .join(name)
            .join("inside"),
    )
    .unwrap();
    let path = dir.path().join("big.bin");
    write_file(&path, &content);

    let stored = store.store_file(&path, 5000, u64::MAX, &no_download());
    assert!(
        matches!(
            &stored,
            Err(Error::History {
                op: "write object",
                ..
            })
        ),
        "{stored:?}"
    );
}

/// A folder found where the walk saw a file is never read as one: after the attempts allowed
/// it is the error of a file that kept changing, and nothing is stored.
#[cfg(unix)]
#[test]
fn a_folder_where_a_file_was_is_not_read() {
    let dir = TestDir::new("history-folder-for-file");
    let (git_dir, mut store) = new_store(&dir);
    let path = dir.path().join("was-a-file");
    write::create_dir(&path).unwrap();

    let stored = store.store_file(&path, 10, u64::MAX, &no_download());
    assert_eq!(
        io_kind(&stored),
        Some(("read", io::ErrorKind::InvalidData)),
        "{stored:?}"
    );
    assert_eq!(files_under(&git_dir.join("objects")), Vec::<PathBuf>::new());
}

/// A read that fails while a large file is hashed in chunks is returned at once, not retried:
/// `/proc/self/mem` is a regular file whose read at offset 0 fails with `EIO`.
#[cfg(target_os = "linux")]
#[test]
fn a_failing_read_of_a_streamed_file_is_returned() {
    let dir = TestDir::new("history-stream-read-error");
    let (_git_dir, mut store) = new_store(&dir);
    store.set_stream_threshold(1);
    let hashed = store.hash_file(Path::new("/proc/self/mem"), 4096, &no_download());
    assert_eq!(
        io_kind(&hashed).map(|(op, _)| op),
        Some("read"),
        "{hashed:?}"
    );
}
