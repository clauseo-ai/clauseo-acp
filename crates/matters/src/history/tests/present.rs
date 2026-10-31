//! Objects found already present when a checkpoint stores them: a sound one is kept as it is,
//! a torn or wrong one is written again. One modified since the last publish is inflated and
//! hashed whole; an older one has its header checked.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gix::objs::{Kind, Write as _};
use gix::odb::loose;
use gix::zlib::Compression;

use super::{git_id, new_store, noise, replace_file, write_file};
use crate::history::{GixStore, HistoryStore, Oid, TIMELINE_REF};
use crate::no_download::NoDownload;
use crate::stat::StatCache;
use crate::test_dir::TestDir;
use crate::write;
use crate::{OsProbe, PlaceholderProbe};

/// A guard for a test's direct reads, with the OS probe.
fn no_download() -> NoDownload<'static> {
    const PROBE: &dyn PlaceholderProbe = &OsProbe;
    NoDownload::begin(PROBE)
}

/// 2020-01-01: a last publish before any object a test writes.
fn long_ago() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_hours(438_288)
}

/// 2100-01-01: a last publish after any object a test writes.
fn far_ahead() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_hours(1_139_568)
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

/// Publishes a first checkpoint, sets the timeline ref's modification time to `at` and opens
/// the store again, so the last publish was at `at`.
fn published_at(dir: &TestDir, git_dir: &Path, mut store: GixStore, at: SystemTime) -> GixStore {
    let tree = store.write_tree(&[]).unwrap();
    let commit = store.write_commit(tree, None, "First", 1).unwrap();
    store
        .publish(commit, None, &mut StatCache::default())
        .unwrap();
    drop(store);
    write::set_mtime(&git_dir.join(TIMELINE_REF), at).unwrap();
    GixStore::open(git_dir, &dir.path().join("folder")).unwrap()
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

/// A sound object that is present but was not written by this run (a crash before its
/// publish) is hashed in full, found sound, and kept: its file is not written again (the store
/// would write it at another zlib level) and it is synced at the next publish.
#[test]
fn a_sound_object_modified_since_the_last_publish_is_kept_as_it_is() {
    for (case, content, compression) in contents() {
        let dir = TestDir::new(&format!("history-present-sound-{case}"));
        let (git_dir, mut store) = new_store(&dir);
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
        assert_reads_back(&store, blob, &content, case);
    }
}

/// A sound object from before the last publish passes its header check and is kept as it is.
#[test]
fn a_sound_object_from_before_the_last_publish_is_kept_as_it_is() {
    let dir = TestDir::new("history-present-old-sound");
    let (git_dir, store) = new_store(&dir);
    let mut store = published_at(&dir, &git_dir, store, far_ahead());
    let content = b"the signed lease\n";
    let id = other_writer(&git_dir, Compression::NONE)
        .write_buf(Kind::Blob, content)
        .unwrap();
    let blob = Oid(id.as_slice().try_into().unwrap());
    let object = object_path(&git_dir, blob);
    let planted = std::fs::read(&object).unwrap();

    assert_eq!(store_content(&dir, &mut store, content), blob);
    assert!(
        std::fs::read(&object).unwrap() == planted,
        "a sound object was written again"
    );
    assert_eq!(store.new_object_files(), [object]);
}

/// An object from before the last publish whose header names another size or another kind
/// fails its header check and is written again, so the blob reads back.
#[test]
fn an_object_from_before_the_last_publish_with_a_wrong_header_is_written_again() {
    let content = b"the signed lease\n";
    let other_size: (&str, Kind, &[u8]) = ("size", Kind::Blob, b"the lease\n");
    let other_kind: (&str, Kind, &[u8]) = ("kind", Kind::Commit, b"the signed lies\n\n");
    for (case, kind, planted) in [other_size, other_kind] {
        let dir = TestDir::new(&format!("history-present-old-header-{case}"));
        let (git_dir, store) = new_store(&dir);
        let mut store = published_at(&dir, &git_dir, store, far_ahead());
        let blob = git_id("blob", content);
        other_writer(&git_dir, Compression::BEST_SPEED)
            .write_buf_with_known_id(kind, planted, gix::ObjectId::Sha1(blob.0))
            .unwrap();

        assert_eq!(store_content(&dir, &mut store, content), blob, "{case}");
        assert_reads_back(&store, blob, content, case);
    }
}

/// After a publish, an object modified since then with the right header but other bytes is
/// hashed in full, found wrong, and written again.
#[test]
fn an_object_modified_since_the_last_publish_with_a_wrong_body_is_written_again() {
    let dir = TestDir::new("history-present-recent-body");
    let (git_dir, store) = new_store(&dir);
    let mut store = published_at(&dir, &git_dir, store, long_ago());
    let content = b"right bytes\n";
    let blob = git_id("blob", content);
    other_writer(&git_dir, Compression::BEST_SPEED)
        .write_buf_with_known_id(Kind::Blob, b"wrong bytes\n", gix::ObjectId::Sha1(blob.0))
        .unwrap();

    assert_eq!(store_content(&dir, &mut store, content), blob);
    assert_reads_back(&store, blob, content, "wrong body");
}

/// An object torn by a crash (cut short) or damaged on disk (one byte flipped in its zlib
/// stream) since the last publish is found unsound and written again.
#[test]
fn a_truncated_or_flipped_object_modified_since_the_last_publish_is_written_again() {
    let truncate: fn(&mut Vec<u8>) = |bytes| bytes.truncate(bytes.len() / 2);
    let flip: fn(&mut Vec<u8>) = |bytes| {
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0x20;
    };
    for (case, damage) in [("truncated", truncate), ("flipped", flip)] {
        for (kind, content, compression) in contents() {
            let case = format!("{case}-{kind}");
            let dir = TestDir::new(&format!("history-present-torn-{case}"));
            let (git_dir, mut store) = new_store(&dir);
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
