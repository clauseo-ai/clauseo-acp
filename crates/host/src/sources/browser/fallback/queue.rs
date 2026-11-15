//! The browser guard's queue of fallback deliveries no agent line told: a file it saved in
//! Clauseo's own downloads folder for the matter in its last sweep (at its exit, or its reaper's
//! after it was killed), when no tool result was left to carry the line. The guard
//! (`clauseo_mcp`'s `browser_guard/downloads/saved.rs`, `queue_fallback`) writes each queue
//! into that folder as `.clauseo-untold-<16 hex digits>`: JSON `{"session": "<thread>-<launch>",
//! "notes": ["Saved ... Its path: <path>", ...]}`. The host reads the folder when a session of the
//! matter ends (its run folder's removal) and when it starts serving (a crash), shows each file
//! once in the session's thread, and deletes the queue files.
//!
//! The queue is no more trusted than the agent line: each note passes [`super::saved_file`]
//! (this folder and one plain name, a regular file by `symlink_metadata`), and the read is
//! bounded: at most [`MAX_FILES`] queue files a read, each at most [`MAX_BYTES`] and
//! [`MAX_NOTES`] notes. A queue file that does not read, or is too big, is deleted unread.

use std::io::Read;
use std::path::{Path, PathBuf};

use clauseo_protocol::ThreadId;
use serde::Deserialize;

/// The start of a queue file's name; then exactly 16 lowercase hex digits.
const PREFIX: &str = ".clauseo-untold-";
/// The most queue files one read takes; the rest wait for the next.
pub(crate) const MAX_FILES: usize = 16;
/// The largest queue file read.
pub(crate) const MAX_BYTES: u64 = 64 * 1024;
/// The most notes read from one queue file.
pub(crate) const MAX_NOTES: usize = 64;

/// One queue file as the guard writes it.
#[derive(Deserialize)]
struct File {
    session: String,
    notes: Vec<String>,
}

/// A file the queue names that passed the check: the thread its session's id names (none when
/// the id names no thread), the file's path and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Queued {
    pub(crate) thread: Option<ThreadId>,
    pub(crate) path: PathBuf,
    pub(crate) name: String,
}

/// Takes the queue files in `folder` (the matter's fallback folder): each is read, deleted,
/// and each of its notes that names a file the guard saved in `folder` is returned. A folder
/// that is missing or not a real folder reads as empty.
pub(crate) fn take(folder: &Path) -> Vec<Queued> {
    if !std::fs::symlink_metadata(folder).is_ok_and(|meta| meta.is_dir()) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let names: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_str().is_some_and(is_queue_name))
        .map(|entry| entry.path())
        .take(MAX_FILES)
        .collect();
    let mut queued = Vec::new();
    for path in names {
        let read = read(&path);
        if let Err(error) = crate::write::remove_old_file(&path) {
            // Not deleted: not trusted to be told only once, so not told.
            tracing::warn!(%error, "a browser download queue file stays");
            continue;
        }
        let Some(file) = read else {
            continue;
        };
        let thread = thread_of(&file.session);
        queued.extend(
            file.notes
                .iter()
                .take(MAX_NOTES)
                .filter_map(|note| super::saved_file(note, folder))
                .map(|(path, name)| Queued { thread, path, name }),
        );
    }
    queued
}

/// Whether `name` is a queue file's: [`PREFIX`] and exactly 16 lowercase hex digits.
fn is_queue_name(name: &str) -> bool {
    name.strip_prefix(PREFIX).is_some_and(|hex| {
        hex.len() == 16
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

/// The queue file at `path`, when it is a regular file (never a link) of at most [`MAX_BYTES`]
/// that reads as one.
fn read(path: &Path) -> Option<File> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    crate::write::open_read_nofollow(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// The thread a session's id (`<thread>-<launch>`, `SessionKey::id`) names.
fn thread_of(session: &str) -> Option<ThreadId> {
    let (thread, launch) = session.rsplit_once('-')?;
    launch.parse::<u64>().ok()?;
    thread.parse().ok()
}

#[cfg(test)]
mod tests;
