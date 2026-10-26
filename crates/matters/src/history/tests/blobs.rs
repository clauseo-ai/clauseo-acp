//! Blobs: the compression rule, hashing, streaming, and objects that exist or are torn.

use std::io;
use std::path::{Path, PathBuf};

use gix::objs::Write as _;

use super::{files_under, git_id, new_store, noise, write_file};
use crate::Error;
use crate::history::blob::is_already_compressed;
use crate::history::{GixStore, HistoryStore, Oid};
use crate::test_dir::TestDir;
use crate::write;

#[test]
fn compressed_formats_are_recognised_by_their_first_bytes() {
    let heads: [&[u8]; 8] = [
        b"PK\x03\x04rest",
        b"%PDF-1.7",
        b"\xFF\xD8\xFF\xE0",
        b"\x89PNG\r\n\x1a\n",
        b"GIF89a",
        b"RIFF\x10\x00\x00\x00WEBPVP8 ",
        b"\x00\x00\x00\x18ftypheic",
        b"\x00\x00\x00\x20ftypmp42",
    ];
    for head in heads {
        assert!(is_already_compressed(head), "{head:?}");
    }
    let plain: [&[u8]; 7] = [
        b"",
        b"PK",
        b"hello world",
        b"RIFF\x10\x00\x00\x00WAVEfmt ",
        b"{\"rtf\": 1}",
        b"<?xml version",
        b"report.pdf",
    ];
    for head in plain {
        assert!(!is_already_compressed(head), "{head:?}");
    }
}

#[test]
fn a_file_above_the_threshold_is_streamed_to_the_same_blob() {
    let dir = TestDir::new("history-stream");
    let (git_dir, mut store) = new_store(&dir);
    let content: Vec<u8> = (0..(5 * 1024 * 1024 / 2))
        .map(|i: u32| (i % 251) as u8)
        .collect();
    let len = content.len() as u64;
    let path = dir.path().join("big.txt");
    write_file(&path, &content);

    let (buffered, read) = store.hash_file(&path, len).unwrap();
    assert_eq!((buffered, read), (git_id("blob", &content), len));
    assert_eq!(files_under(&git_dir.join("objects")), Vec::<PathBuf>::new());

    store.set_stream_threshold(1024);
    let (hashed, _) = store.hash_file(&path, len).unwrap();
    let (streamed, read) = store.store_file(&path, len, u64::MAX).unwrap();
    assert_eq!((hashed, streamed, read), (buffered, buffered, len));
    assert_eq!(files_under(&git_dir.join("objects")).len(), 1);
    assert_eq!(store.blob_len(streamed).unwrap(), len);
    let mut out = Vec::new();
    assert_eq!(store.read_blob(streamed, &mut out).unwrap(), len);
    assert!(out == content, "the blob reads back different bytes");
}

#[test]
fn a_wrong_length_is_read_again_from_the_file() {
    let dir = TestDir::new("history-wrong-length");
    let (_git_dir, mut store) = new_store(&dir);
    let path = dir.path().join("note.txt");
    write_file(&path, b"twelve bytes");
    for (threshold, guess) in [(u64::MAX, 3), (4, 30)] {
        store.set_stream_threshold(threshold);
        let (blob, read) = store.store_file(&path, guess, u64::MAX).unwrap();
        assert_eq!((blob, read), (git_id("blob", b"twelve bytes"), 12));
    }
}

#[test]
fn storing_an_existing_object_creates_no_file() {
    let dir = TestDir::new("history-existing");
    let (git_dir, mut store) = new_store(&dir);
    let path = dir.path().join("note.txt");
    write_file(&path, b"same bytes\n");
    let first = store.store_file(&path, 11, u64::MAX).unwrap();
    let files = files_under(&git_dir.join("objects"));
    assert_eq!(files.len(), 1);
    assert_eq!(store.new_object_files(), files.as_slice());

    let second = store.store_file(&path, 11, u64::MAX).unwrap();
    assert_eq!(first, second);
    assert_eq!(files_under(&git_dir.join("objects")), files);
    assert_eq!(store.new_object_files(), files.as_slice());
}

/// The loose object path of `blob` under `git_dir`.
fn object_path(git_dir: &Path, blob: Oid) -> PathBuf {
    let hex = blob.to_hex();
    let (folder, name) = hex.split_at(2);
    git_dir.join("objects").join(folder).join(name)
}

#[test]
fn a_torn_object_is_written_again() {
    for (case, planted) in [("empty", b"".as_slice()), ("garbage", b"not zlib at all")] {
        let dir = TestDir::new(&format!("history-torn-{case}"));
        let (git_dir, mut store) = new_store(&dir);
        let content = b"the signed lease\n";
        let blob = git_id("blob", content);
        let object = object_path(&git_dir, blob);
        write::create_dir(object.parent().unwrap()).unwrap();
        write_file(&object, planted);
        let path = dir.path().join("lease.txt");
        write_file(&path, content);

        assert_eq!(
            store.store_file(&path, 17, u64::MAX).unwrap(),
            (blob, 17),
            "{case}"
        );
        let mut out = Vec::new();
        store.read_blob(blob, &mut out).unwrap();
        assert_eq!(out, content, "{case}");
        assert_eq!(store.new_object_files(), [object], "{case}");
    }
}

#[test]
fn a_present_object_is_synced_at_the_next_publish() {
    let dir = TestDir::new("history-present-synced");
    let (git_dir, mut store) = new_store(&dir);
    let path = dir.path().join("note.txt");
    write_file(&path, b"kept bytes\n");
    let (blob, _) = store.store_file(&path, 11, u64::MAX).unwrap();
    // A run that wrote the object and stopped before publishing.
    drop(store);

    let mut store = GixStore::open(&git_dir, &dir.path().join("folder")).unwrap();
    assert_eq!(store.new_object_files(), Vec::<PathBuf>::new());
    assert_eq!(store.store_file(&path, 11, u64::MAX).unwrap(), (blob, 11));
    assert_eq!(store.new_object_files(), [object_path(&git_dir, blob)]);
}

#[test]
fn a_present_object_with_a_wrong_body_is_written_again() {
    let dir = TestDir::new("history-present-wrong-body");
    let (git_dir, mut store) = new_store(&dir);
    let content = b"right bytes\n";
    let blob = git_id("blob", content);
    // The same kind and size, so only inflating and hashing the whole object tells.
    let objects = gix::odb::loose::Store::at(git_dir.join("objects"), gix::hash::Kind::Sha1);
    let id = gix::ObjectId::Sha1(blob.0);
    objects
        .write_buf_with_known_id(gix::objs::Kind::Blob, b"wrong bytes\n", id)
        .unwrap();
    let path = dir.path().join("note.txt");
    write_file(&path, content);

    assert_eq!(store.store_file(&path, 12, u64::MAX).unwrap(), (blob, 12));
    let mut out = Vec::new();
    store.read_blob(blob, &mut out).unwrap();
    assert_eq!(out, content);
}

#[test]
fn a_file_grown_over_the_cap_is_refused() {
    let dir = TestDir::new("history-over-cap");
    let (git_dir, mut store) = new_store(&dir);
    let path = dir.path().join("grown.txt");
    write_file(&path, &[b'x'; 30]);

    // The walk saw 10 bytes under a cap of 20; the file now holds 30.
    let refused = store.store_file(&path, 10, 20);
    let kind = match &refused {
        Err(Error::Io { source, .. }) => Some(source.kind()),
        _ => None,
    };
    assert_eq!(kind, Some(io::ErrorKind::InvalidData), "{refused:?}");
    assert_eq!(files_under(&git_dir.join("objects")), Vec::<PathBuf>::new());
}

#[test]
fn compressed_files_stored_at_level_0() {
    let dir = TestDir::new("history-level-0");
    let (git_dir, mut store) = new_store(&dir);
    let mut zip = b"PK\x03\x04".to_vec();
    zip.extend(noise(64 * 1024));
    let text = "The tenant shall pay the rent monthly.\n"
        .repeat(2000)
        .into_bytes();
    let zip_path = dir.path().join("brief.docx");
    let text_path = dir.path().join("brief.txt");
    write_file(&zip_path, &zip);
    write_file(&text_path, &text);
    let (zip_blob, _) = store
        .store_file(&zip_path, zip.len() as u64, u64::MAX)
        .unwrap();
    let (text_blob, _) = store
        .store_file(&text_path, text.len() as u64, u64::MAX)
        .unwrap();

    let raw = |blob: Oid| {
        let hex = blob.to_hex();
        let (folder, name) = hex.split_at(2);
        std::fs::read(git_dir.join("objects").join(folder).join(name)).unwrap()
    };
    let (zip_raw, text_raw) = (raw(zip_blob), raw(text_blob));
    // Byte 2 opens the first deflate block, after the 2-byte zlib header: bit 0 is BFINAL,
    // bits 1-2 are BTYPE, 00 for a stored (uncompressed) block.
    assert_eq!(
        (zip_raw[2] >> 1) & 0b11,
        0,
        "the zip's first block is not stored"
    );
    assert_ne!(
        (text_raw[2] >> 1) & 0b11,
        0,
        "the text's first block is stored"
    );
    assert!(
        zip_raw.len() >= zip.len(),
        "{} < {}",
        zip_raw.len(),
        zip.len()
    );
    assert!(
        text_raw.len() * 10 < text.len(),
        "{} vs {}",
        text_raw.len(),
        text.len()
    );
}
