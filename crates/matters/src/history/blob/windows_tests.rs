//! Windows: files read whole or streamed, a length over the cap, a symlink where a file was, a
//! read that fails while a file is hashed, and an object that cannot be written. Windows has no
//! `O_NOFOLLOW`, no `/proc/self/mem` and no `NotADirectory` for a file inside a file, so the Unix
//! tests of these paths (`history/tests/io_errors.rs`) do not run here.

use std::io;
use std::io::Write as _;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use super::{Blobs, Target};
use crate::Error;
use crate::history::Oid;
use crate::history::durable::NewObjects;
use crate::history::gix_store::to_oid;
use crate::history::objects::Objects;
use crate::no_download::{NoDownload, fault};
use crate::test_dir::TestDir;
use crate::{OsProbe, PlaceholderProbe, write};

/// A guard for a test's reads, with the OS probe.
fn guard() -> NoDownload<'static> {
    const PROBE: &dyn PlaceholderProbe = &OsProbe;
    NoDownload::begin(PROBE)
}

/// An empty loose object store at `<dir>/objects`.
fn objects(dir: &TestDir) -> Objects {
    let path = dir.path().join("objects");
    write::create_dir_all(&path).unwrap();
    Objects::at(&path)
}

/// Whether `objects` holds nothing: no `objects/xx` folder, so no object was written.
fn is_empty(objects: &Objects) -> bool {
    std::fs::read_dir(objects.path()).unwrap().next().is_none()
}

fn write_file(path: &Path, bytes: &[u8]) {
    write::create_new(path).unwrap().write_all(bytes).unwrap();
}

/// Deterministic bytes that do not compress.
fn noise(len: usize) -> Vec<u8> {
    let mut state = 0x2545_f491_u32;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect()
}

/// The id git gives a blob of `data`.
fn blob_id(data: &[u8]) -> Oid {
    let id = gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, data).unwrap();
    to_oid(&id).unwrap()
}

/// Stores the file `path`, `len` bytes by the walk's stat, under `cap`, in `objects`.
fn store(
    blobs: &mut Blobs,
    objects: &Objects,
    path: &Path,
    len: u64,
    cap: u64,
) -> Result<(Oid, u64), Error> {
    let mut new_objects = NewObjects::default();
    let target = Target {
        stored: objects.stored(),
        deflated: objects.deflated(),
        new_objects: &mut new_objects,
    };
    blobs.read(path, len, cap, Some(target), &guard())
}

/// The `op` and kind of an [`Error::Io`], or `None` for any other result.
fn io_kind<T>(result: &Result<T, Error>) -> Option<(&'static str, io::ErrorKind)> {
    match result {
        Err(Error::Io { op, source, .. }) => Some((op, source.kind())),
        _ => None,
    }
}

/// A 2 MiB file, under the 8 MiB threshold, is read whole in one pass: a change armed for the
/// moment a stream's second pass would start never happens, and the blob is the bytes read.
#[test]
fn a_file_under_the_threshold_is_read_whole_in_one_pass() {
    let dir = TestDir::new("blob-win-threshold");
    let objects = objects(&dir);
    let first = noise(2 * 1024 * 1024);
    let mut second = first.clone();
    second[1000] ^= 0xff;
    let len = first.len() as u64;
    let path = dir.path().join("big.bin");
    write_file(&path, &first);
    fault::change_between_passes(&path, &second);

    let stored = store(&mut Blobs::new(), &objects, &path, len, u64::MAX);
    fault::clear();

    assert_eq!(stored.unwrap(), (blob_id(&first), len));
    assert!(
        std::fs::read(&path).unwrap() == first,
        "the file was streamed: the change between passes ran"
    );
}

/// A streamed file is hashed in 1 MiB chunks: 2 MiB takes three read calls (two chunks and the
/// end), well within eight, where 1 KiB chunks would take over two thousand.
#[test]
fn a_streamed_file_is_hashed_in_large_chunks() {
    let dir = TestDir::new("blob-win-chunks");
    let content = noise(2 * 1024 * 1024);
    let len = content.len() as u64;
    let path = dir.path().join("big.bin");
    write_file(&path, &content);
    let mut blobs = Blobs::new();
    blobs.set_stream_threshold(1024);
    fault::fail(&path, 8, io::ErrorKind::Deadlock);

    let hashed = blobs.read(&path, len, u64::MAX, None, &guard());
    let failed = fault::failed(&path);
    fault::clear();

    assert_eq!(failed, 0, "more than eight read calls");
    assert_eq!(hashed.unwrap(), (blob_id(&content), len));
}

/// A length the walk saw over the cap is refused at once, before the file is read, even though
/// the file now holds less; nothing is stored.
#[test]
fn a_length_seen_over_the_cap_is_refused_before_the_read() {
    let dir = TestDir::new("blob-win-seen-over-cap");
    let objects = objects(&dir);
    let path = dir.path().join("shrunk.txt");
    write_file(&path, &[b'x'; 10]);

    // The walk saw 30 bytes under a cap of 20; the file now holds 10.
    let refused = store(&mut Blobs::new(), &objects, &path, 30, 20);
    assert_eq!(
        io_kind(&refused),
        Some(("read", io::ErrorKind::FileTooLarge)),
        "{refused:?}"
    );
    assert!(is_empty(&objects), "an object was stored");
}

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
/// The read runs on its own thread (faults are per thread) under a deadline, so a retry loop
/// fails the test instead of hanging it.
#[test]
fn a_failing_read_while_a_file_is_hashed_is_returned() {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let dir = TestDir::new("blob-win-hash-read-error");
        let path = dir.path().join("big.bin");
        write_file(&path, &noise(5000));
        let mut blobs = Blobs::new();
        blobs.set_stream_threshold(1024);
        fault::fail(&path, 0, io::ErrorKind::Deadlock);
        let hashed = blobs.read(&path, 5000, u64::MAX, None, &guard());
        let failed = fault::failed(&path);
        fault::clear();
        send.send((hashed.map_err(|err| err.is_not_downloaded()), failed))
            .unwrap();
    });

    let (hashed, failed) = receive
        .recv_timeout(Duration::from_secs(60))
        .expect("the failing read was retried, not returned");
    assert_eq!(hashed, Err(true), "not the read's error");
    assert_eq!(failed, 1);
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
