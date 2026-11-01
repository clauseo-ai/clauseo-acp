//! Objects found already present when a checkpoint stores them: a sound one is kept as it is,
//! a torn or wrong one is written again. Each is inflated whole however old its file is (no
//! test sets a last publish: nothing reads its time), and checked against the bytes in hand
//! when the file was read whole, or hashed against its id when it was streamed.

use std::path::{Path, PathBuf};

use gix::objs::{Kind, Write as _};
use gix::odb::loose;
use gix::zlib::Compression;

use super::{git_id, new_store, noise, replace_file, write_file};
use crate::history::{GixStore, HistoryStore, Oid};
use crate::no_download::NoDownload;
use crate::test_dir::TestDir;
use crate::{OsProbe, PlaceholderProbe};

/// A guard for a test's direct reads, with the OS probe.
fn no_download() -> NoDownload<'static> {
    const PROBE: &dyn PlaceholderProbe = &OsProbe;
    NoDownload::begin(PROBE)
}

/// A loose store on the objects of `git_dir` writing at `compression`: another writer (an older
/// version, a sync tool, a restored backup), whose files the store has not written itself.
fn other_writer(git_dir: &Path, compression: Compression) -> loose::Store {
    let options = loose::Options {
        alloc_limit_bytes: None,
        compression,
    };
    loose::Store::at_opts(git_dir.join("objects"), gix::hash::Kind::Sha1, options)
}

/// The loose object file of `blob` under `git_dir`.
fn object_path(git_dir: &Path, blob: Oid) -> PathBuf {
    let hex = blob.to_hex();
    let (folder, name) = hex.split_at(2);
    git_dir.join("objects").join(folder).join(name)
}

/// Stores a file holding `content` and checks the blob it names.
fn store_content(dir: &TestDir, store: &mut GixStore, content: &[u8]) -> Oid {
    let path = dir.path().join("file.bin");
    if path.exists() {
        replace_file(&path, content);
    } else {
        write_file(&path, content);
    }
    let len = content.len() as u64;
    let stored = store
        .store_file(&path, len, u64::MAX, &no_download())
        .unwrap();
    assert_eq!(stored, (git_id("blob", content), len));
    stored.0
}

/// Checks that `blob` reads back as `content`.
fn assert_reads_back(store: &GixStore, blob: Oid, content: &[u8], case: &str) {
    let mut out = Vec::new();
    let read = store.read_blob(blob, "file.bin", &mut out);
    assert!(
        read.is_ok() && out == content,
        "{case}: the blob does not read back: {read:?}"
    );
}

/// Contents to plant: a short text, incompressible bytes over several inflate chunks, and a
/// long repetitive text whose few compressed bytes inflate past several chunks.
fn contents() -> [(&'static str, Vec<u8>, Compression); 3] {
    let repetitive = "The tenant shall pay the rent monthly.\n"
        .repeat(50_000)
        .into_bytes();
    [
        ("short", b"the signed lease\n".to_vec(), Compression::NONE),
        ("noise", noise(300 * 1024), Compression::NONE),
        ("repetitive", repetitive, Compression::BEST),
    ]
}

/// The two ways a file reaches the store: read whole, so its bytes are in hand and a present
/// object is compared with them, or streamed (the threshold set below it), so a present object
/// is hashed against its id.
const PATHS: [(&str, u64); 2] = [("in-hand", u64::MAX), ("streamed", 1)];

/// A sound object that is present but was not written by this run (a crash before its
/// publish, or a version from before it that a revert names again) is inflated in full, found
/// sound, and kept: its file is not written again (the store would write it at another zlib
/// level) and it is synced at the next publish. On both paths.
#[test]
fn a_sound_present_object_is_kept_as_it_is() {
    for (path, threshold) in PATHS {
        for (kind, content, compression) in contents() {
            let case = format!("{kind}-{path}");
            let dir = TestDir::new(&format!("history-present-sound-{case}"));
            let (git_dir, mut store) = new_store(&dir);
            store.set_stream_threshold(threshold);
            let id = other_writer(&git_dir, compression)
                .write_buf(Kind::Blob, &content)
                .unwrap();
            let blob = Oid(id.as_slice().try_into().unwrap());
            let object = object_path(&git_dir, blob);
            let planted = std::fs::read(&object).unwrap();

            assert_eq!(store_content(&dir, &mut store, &content), blob, "{case}");
            assert!(
                std::fs::read(&object).unwrap() == planted,
                "{case}: a sound object was written again"
            );
            assert_eq!(store.new_object_files(), [object], "{case}");
            assert_reads_back(&store, blob, &content, &case);
        }
    }
}

/// A present object whose header names another size or another kind fails its check and is
/// written again, so the blob reads back. On both paths.
#[test]
fn a_present_object_with_a_wrong_header_is_written_again() {
    let content = b"the signed lease\n";
    let other_size: (&str, Kind, &[u8]) = ("size", Kind::Blob, b"the lease\n");
    let other_kind: (&str, Kind, &[u8]) = ("kind", Kind::Commit, b"the signed lies\n\n");
    for (path, threshold) in PATHS {
        for (header, kind, planted) in [other_size, other_kind] {
            let case = format!("{header}-{path}");
            let dir = TestDir::new(&format!("history-present-header-{case}"));
            let (git_dir, mut store) = new_store(&dir);
            store.set_stream_threshold(threshold);
            let blob = git_id("blob", content);
            other_writer(&git_dir, Compression::BEST_SPEED)
                .write_buf_with_known_id(kind, planted, gix::ObjectId::Sha1(blob.0))
                .unwrap();

            assert_eq!(store_content(&dir, &mut store, content), blob, "{case}");
            assert_reads_back(&store, blob, content, &case);
        }
    }
}

/// A present object with the right header (same kind and size) but other bytes, as a disk or a
/// sync tool could leave it, is inflated in full, found wrong, and written again: compared with
/// the bytes in hand when the file was read whole, hashed against its id when it was streamed.
#[test]
fn a_present_object_with_the_right_header_and_a_wrong_body_is_written_again() {
    for (case, threshold) in PATHS {
        let dir = TestDir::new(&format!("history-present-body-{case}"));
        let (git_dir, mut store) = new_store(&dir);
        store.set_stream_threshold(threshold);
        let content = b"right bytes\n";
        let blob = git_id("blob", content);
        other_writer(&git_dir, Compression::BEST_SPEED)
            .write_buf_with_known_id(Kind::Blob, b"wrong bytes\n", gix::ObjectId::Sha1(blob.0))
            .unwrap();

        assert_eq!(store_content(&dir, &mut store, content), blob, "{case}");
        assert_reads_back(&store, blob, content, case);
    }
}

/// An object torn by a crash (cut short) or damaged on disk (one byte flipped in its zlib
/// stream) is found unsound and written again, on both paths.
#[test]
fn a_truncated_or_flipped_present_object_is_written_again() {
    let truncate: fn(&mut Vec<u8>) = |bytes| bytes.truncate(bytes.len() / 2);
    let flip: fn(&mut Vec<u8>) = |bytes| {
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0x20;
    };
    let cases = [("truncated", truncate), ("flipped", flip)];
    for ((case, damage), (path, threshold)) in cases.into_iter().flat_map(|c| PATHS.map(|p| (c, p)))
    {
        for (kind, content, compression) in contents() {
            let case = format!("{case}-{kind}-{path}");
            let dir = TestDir::new(&format!("history-present-torn-{case}"));
            let (git_dir, mut store) = new_store(&dir);
            store.set_stream_threshold(threshold);
            let id = other_writer(&git_dir, compression)
                .write_buf(Kind::Blob, &content)
                .unwrap();
            let blob = Oid(id.as_slice().try_into().unwrap());
            let object = object_path(&git_dir, blob);
            let mut damaged = std::fs::read(&object).unwrap();
            damage(&mut damaged);
            replace_file(&object, &damaged);

            assert_eq!(store_content(&dir, &mut store, &content), blob, "{case}");
            assert_reads_back(&store, blob, &content, &case);
            assert_eq!(store.new_object_files(), [object], "{case}");
        }
    }
}
