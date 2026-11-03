//! Blobs read whole or streamed, by the threshold and the hash chunk, and a length the walk saw
//! over the cap; on every OS. The Windows-only cases are in `windows_tests.rs`.

use std::io;
use std::io::Write as _;
use std::path::Path;

use super::{Blobs, HEAD_BYTES, Target, extend_head};
use crate::Error;
use crate::history::Oid;
use crate::history::durable::NewObjects;
use crate::history::gix_store::to_oid;
use crate::history::objects::Objects;
use crate::no_download::{NoDownload, fault};
use crate::test_dir::TestDir;
use crate::{OsProbe, PlaceholderProbe, write};

/// A guard for a test's reads, with the OS probe.
pub(super) fn guard() -> NoDownload<'static> {
    const PROBE: &dyn PlaceholderProbe = &OsProbe;
    NoDownload::begin(PROBE)
}

/// An empty loose object store at `<dir>/objects`.
pub(super) fn objects(dir: &TestDir) -> Objects {
    let path = dir.path().join("objects");
    write::create_dir_all(&path).unwrap();
    Objects::at(&path)
}

/// Whether `objects` holds nothing: no `objects/xx` folder, so no object was written.
pub(super) fn is_empty(objects: &Objects) -> bool {
    std::fs::read_dir(objects.path()).unwrap().next().is_none()
}

pub(super) fn write_file(path: &Path, bytes: &[u8]) {
    write::create_new(path).unwrap().write_all(bytes).unwrap();
}

/// Deterministic bytes that do not compress.
pub(super) fn noise(len: usize) -> Vec<u8> {
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
pub(super) fn blob_id(data: &[u8]) -> Oid {
    let id = gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, data).unwrap();
    to_oid(&id).unwrap()
}

/// Stores the file `path`, `len` bytes by the walk's stat, under `cap`, in `objects`.
pub(super) fn store(
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
pub(super) fn io_kind<T>(result: &Result<T, Error>) -> Option<(&'static str, io::ErrorKind)> {
    match result {
        Err(Error::Io { op, source, .. }) => Some((op, source.kind())),
        _ => None,
    }
}

/// A 2 MiB file, under the 8 MiB threshold, is read whole in one pass: a change armed for the
/// moment a stream's second pass would start never happens, and the blob is the bytes read.
#[test]
fn a_file_under_the_threshold_is_read_whole_in_one_pass() {
    let dir = TestDir::new("blob-threshold");
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
    let dir = TestDir::new("blob-chunks");
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
    let dir = TestDir::new("blob-seen-over-cap");
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

/// A streamed file's head, the first bytes that choose its zlib level, stays those first bytes
/// however many chunks the file is read in, short or long: it never grows with the file, which
/// would hold a large file in memory. (No test sees memory use, so it is checked here.)
#[test]
fn a_streamed_files_head_stays_its_first_bytes_over_many_chunks() {
    let content = noise(64 * 1024);
    for size in [1, 5, HEAD_BYTES, 1000] {
        let mut head = Vec::new();
        for chunk in content.chunks(size) {
            extend_head(&mut head, chunk);
        }
        assert_eq!(head, content[..HEAD_BYTES], "chunks of {size} bytes");
    }
}
