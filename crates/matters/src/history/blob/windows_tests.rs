//! Windows: a symlink where a file was, a read that fails while a file is hashed, and an
//! object that cannot be written. Windows has no `O_NOFOLLOW`, no `/proc/self/mem` and no
//! `NotADirectory` for a file inside a file, so the Unix tests of these paths
//! (`history/tests/io_errors.rs`) do not run here.

use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use super::Blobs;
use super::tests::{blob_id, guard, io_kind, is_empty, noise, objects, store, write_file};
use crate::Error;
use crate::test_dir::TestDir;

#[expect(
    clippy::disallowed_methods,
    reason = "the blob test makes a symlink to check it is not read through"
)]
fn symlink(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_file(target, link)
        .expect("a symlink (the Windows runners run as administrator)");
}

/// A symlink where the walk saw a file is never read through. Windows' open follows it, so the
/// open file is a regular file while the name is a symlink; after the attempts allowed it is a
/// file that kept changing, and nothing is stored.
#[test]
fn a_symlink_where_a_file_was_is_not_read_through() {
    let dir = TestDir::new("blob-win-symlink");
    let objects = objects(&dir);
    let target = dir.path().join("target.txt");
    write_file(&target, b"twelve bytes");
    let link = dir.path().join("link.txt");
    symlink(&target, &link);

    let stored = store(&mut Blobs::new(), &objects, &link, 12, u64::MAX);
    assert_eq!(
        io_kind(&stored),
        Some(("read", io::ErrorKind::InvalidData)),
        "{stored:?}"
    );
    assert!(is_empty(&objects), "an object was stored");
}

/// A read that fails while a large file is hashed in chunks is returned at once, not retried.
/// Another handle holds an exclusive lock on the whole file (`LockFileEx`), so the open and the
/// stat succeed and every `ReadFile` fails with `ERROR_LOCK_VIOLATION`. That is a real read
/// error from the chunk loop's `read`, which the test-only fault hook (returned before that
/// `read`) is not. The read runs on its own thread under a deadline, so a retry loop fails the
/// test instead of hanging it.
#[test]
fn a_failing_read_while_a_file_is_hashed_is_returned() {
    let dir = TestDir::new("blob-win-hash-read-error");
    let path = dir.path().join("big.bin");
    write_file(&path, &noise(5000));
    let holder = File::open(&path).unwrap();
    holder.lock().unwrap();

    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut blobs = Blobs::new();
        blobs.set_stream_threshold(1024);
        let hashed = blobs.read(&path, 5000, u64::MAX, None, &guard());
        send.send(io_kind(&hashed)).unwrap();
    });

    let hashed = receive
        .recv_timeout(Duration::from_secs(60))
        .expect("the failing read was retried, not returned");
    drop(holder);
    // The read's own error, not the "kept changing" one (also "read", but `InvalidData`).
    assert!(
        matches!(hashed, Some(("read", kind)) if kind != io::ErrorKind::InvalidData),
        "not the read's error: {hashed:?}"
    );
}

/// A streamed file whose object cannot be written (a file stands where its `objects/xx`
/// folder goes) is the write's error: not a file that kept changing, read again.
#[test]
fn a_streamed_file_whose_object_cannot_be_written_is_a_write_error() {
    let dir = TestDir::new("blob-win-stream-unwritable");
    let objects = objects(&dir);
    let content = noise(5000);
    let hex = blob_id(&content).to_hex();
    let (folder, _) = hex.split_at(2);
    write_file(&objects.path().join(folder), b"not a folder");
    let path = dir.path().join("big.bin");
    write_file(&path, &content);
    let mut blobs = Blobs::new();
    blobs.set_stream_threshold(1024);

    let stored = store(&mut blobs, &objects, &path, 5000, u64::MAX);
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
