//! Tests of [`GixStore`]: the on-disk format, objects, trees, commits, the timeline ref and the
//! stat cache.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::{GixStore, HistoryStore, Oid, TreeFile};
use crate::test_dir::TestDir;
use crate::write;

mod blobs;
mod cache;
mod io_errors;
mod present;
mod refusals;
mod repo;
mod trees;

/// A new store in `dir`: `root/matter/history.git`, worktree `folder`.
fn new_store(dir: &TestDir) -> (PathBuf, GixStore) {
    let git_dir = dir.path().join("root").join("matter").join("history.git");
    let store = GixStore::create(&git_dir, &dir.path().join("folder")).unwrap();
    (git_dir, store)
}

fn write_file(path: &Path, bytes: &[u8]) {
    let mut file = write::create_new(path).unwrap();
    file.write_all(bytes).unwrap();
}

fn replace_file(path: &Path, bytes: &[u8]) {
    write::replace(path, &mut |file| {
        file.write_all(bytes).unwrap();
        Ok(())
    })
    .unwrap();
}

/// Every file under `dir`, sorted.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    files
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

/// The id git gives an object of `kind` (`blob`, `tree`, `commit`) with `data`.
fn git_id(kind: &str, data: &[u8]) -> Oid {
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(format!("{kind} {}\0", data.len()).as_bytes());
    hasher.update(data);
    let id = hasher.try_finalize().unwrap();
    Oid(id.as_slice().try_into().unwrap())
}

fn tree_entry(mode: &str, name: &str, oid: Oid) -> Vec<u8> {
    let mut bytes = format!("{mode} {name}\0").into_bytes();
    bytes.extend_from_slice(&oid.0);
    bytes
}

fn file(path: &str, byte: u8) -> TreeFile {
    TreeFile {
        path: path.to_owned(),
        blob: Oid([byte; 20]),
    }
}
