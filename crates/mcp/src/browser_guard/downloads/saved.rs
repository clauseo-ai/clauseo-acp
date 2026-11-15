//! What the agent hears about the files the mover delivered. Each file moved into the downloads
//! folder (`<matter>/downloads` in a Codex session, the guard's `--downloads`) is recorded by
//! the name the mover gave it ([`Saved::record`]); the next tool call's result the guard relays
//! gets one more text block per file (`Saved {name} to downloads/ in the matter.`), as the
//! held-download note does (`held.rs`), so the agent learns the file's path in the matter.
//!
//! An `agent_browser_open` of a URL Chrome saves rather than shows (a PDF link, with
//! `plugins.always_open_pdf_externally`; any `Content-Disposition: attachment`) fails in
//! agent-browser with `net::ERR_ABORTED`, as the navigation turned into a download. The guard
//! answers that call itself ([`open_reply`]): it waits up to [`OPEN_WAIT`] for the mover, then
//! says the file was saved, or that the download is still in progress, as a success. With no
//! download in sight by then, agent-browser's error goes on as it was.
//!
//! Notes still untold when the guard exits ([`Saved::drain`]), and those of the reaper's last
//! sweep after a guard was killed, are queued in the guard's `--cwd` ([`queue`]: one file
//! `.clauseo-untold-<16 hex digits>` each, a JSON list of notes, written under a temporary
//! name and renamed), and the next guard under that `--cwd` (an MCP reconnect, or the session
//! resumed) tells them with its next tool result ([`Saved::queued_in`]), so the host still sees
//! a file delivered to the fallback folder and shows the lawyer where it is.
//!
//! A session that never resumes has no next guard, so the fallback deliveries among those notes
//! are also queued in the fallback folder itself ([`queue_fallback`]: the same file name, JSON
//! `{"session": "<--cwd's name>", "notes": [...]}`), which the host reads when the session ends
//! and when it starts, and shows the lawyer each file there once (`crates/host`,
//! `sources/browser/fallback/queue.rs`). Each queue file is made hidden (`write::Folder::
//! create_hidden`).

use std::ffi::{OsStr, OsString};
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

use crate::browser_guard::pause;
use crate::write;

/// How long an `open` that ended in `net::ERR_ABORTED` waits for its download to be delivered.
pub(crate) const OPEN_WAIT: Duration = Duration::from_secs(5);
/// How often that wait looks.
const OPEN_POLL: Duration = Duration::from_millis(100);
/// Chrome's error for a navigation that turned into a download.
const ABORTED: &str = "net::ERR_ABORTED";
/// What an `open` says when its download is still being written at the end of [`OPEN_WAIT`].
pub(crate) const IN_PROGRESS: &str = "The page is a file, and the browser is still downloading \
    it. When the download finishes, Clauseo saves it to downloads/ in the matter, and a later \
    browser tool result names the file.";

/// The delivered files the agent has not been told about yet.
#[derive(Default)]
pub(crate) struct Saved {
    untold: Mutex<Vec<String>>,
    /// The guard's `--cwd`, where notes an earlier guard or reaper queued wait ([`queue`]);
    /// read before each telling. None in the tests that do not queue.
    queue: Option<PathBuf>,
}

impl Saved {
    /// A record that also tells the notes queued in `cwd` ([`queue`]).
    pub(crate) fn queued_in(cwd: PathBuf) -> Saved {
        Saved {
            untold: Mutex::default(),
            queue: Some(cwd),
        }
    }

    /// The notes queued in [`Saved::queue`], each file's taken once, added to the untold ones.
    fn pull(&self, untold: &mut Vec<String>) {
        if let Some(cwd) = &self.queue {
            untold.extend(take_queued(cwd));
        }
    }

    /// The notes untold so far, without the queued ones: the guard's own, to [`queue`] at its
    /// exit.
    pub(crate) fn drain(&self) -> Vec<String> {
        std::mem::take(&mut *lock(&self.untold))
    }

    /// One more file delivered, under `name`, in the downloads folder.
    pub(crate) fn record(&self, name: &str) {
        lock(&self.untold).push(note(name));
    }

    /// One more file that stays in staging for good, with `why`.
    pub(crate) fn record_unsaved(&self, why: String) {
        lock(&self.untold).push(why);
    }

    /// One more file delivered outside the matter, as `name` at `path` in the fallback folder
    /// (`deliver.rs`): told with [`fallback_note`], which the host reads.
    pub(crate) fn record_fallback(&self, name: &str, path: &Path) {
        lock(&self.untold).push(fallback_note(name, path));
    }

    /// The notes untold so far, which are told now.
    pub(crate) fn take(&self) -> Vec<String> {
        let mut untold = lock(&self.untold);
        self.pull(&mut untold);
        std::mem::take(&mut *untold)
    }

    /// `line`, a reply to a tool call, with one text block per untold note appended to its
    /// `result.content` (made when the result has none); they are told then. `None` (nothing
    /// told) when none is untold, or the line has no result object (an error reply) or a
    /// `content` that is no list: the notes wait for the next result.
    pub(crate) fn note_into(&self, line: &[u8]) -> Option<Vec<u8>> {
        let mut untold = lock(&self.untold);
        self.pull(&mut untold);
        if untold.is_empty() {
            return None;
        }
        let mut reply: Value = serde_json::from_slice(line).ok()?;
        let content = reply
            .get_mut("result")?
            .as_object_mut()?
            .entry("content")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()?;
        for text in untold.iter() {
            content.push(serde_json::json!({"type": "text", "text": text}));
        }
        let bytes = serde_json::to_vec(&reply).ok()?;
        untold.clear();
        Some(bytes)
    }
}

/// The start of a queued notes file's name; then 16 lowercase hex digits.
const QUEUE_PREFIX: &str = ".clauseo-untold-";
/// The end of the name a queued notes file is written under before its rename.
const QUEUE_WRITING: &str = ".new";
/// How many names a queue file tries when each is taken.
const QUEUE_TRIES: usize = 8;

/// Queues `notes` in `cwd` for the next guard there to tell (above): written to
/// `.clauseo-untold-<random>.new`, made new through the folder's handle, then renamed without
/// replacing to `.clauseo-untold-<random>`, so a reader never sees half a file. Noted on stderr
/// when that fails: the notes are then lost.
pub(crate) fn queue(cwd: &Path, notes: &[String]) {
    if notes.is_empty() {
        return;
    }
    let written = serde_json::to_vec(notes)
        .map_err(std::io::Error::other)
        .and_then(|bytes| try_queue(cwd, &bytes));
    if let Err(error) = written {
        crate::browser_guard::note(&format!(
            "{} note(s) to the agent could not be queued for the next guard: {error}",
            notes.len()
        ));
    }
}

/// Queues the fallback deliveries among `notes` (each [`fallback_note`]) in `fallback`, the
/// fallback folder, for the host (above), naming the session by `cwd`'s name. Nothing when
/// none is a fallback delivery; noted on stderr when the write fails.
pub(crate) fn queue_fallback(fallback: &Path, cwd: &Path, notes: &[String]) {
    let delivered: Vec<&String> = notes
        .iter()
        .filter(|note| note.starts_with("Saved ") && note.contains(FALLBACK_MARKER))
        .collect();
    if delivered.is_empty() {
        return;
    }
    let session = cwd
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let written = serde_json::to_vec(&serde_json::json!({
        "session": session,
        "notes": delivered,
    }))
    .map_err(std::io::Error::other)
    .and_then(|bytes| try_queue(fallback, &bytes));
    if let Err(error) = written {
        crate::browser_guard::note(&format!(
            "{} download(s) saved outside the matter could not be queued for the host: {error}",
            delivered.len()
        ));
    }
}

/// Writes `bytes` into `folder` as a new queue file (above), hidden on Windows.
fn try_queue(folder: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let folder = write::Folder::open(folder)?;
    let mut last = std::io::Error::from(std::io::ErrorKind::AlreadyExists);
    for _ in 0..QUEUE_TRIES {
        let random = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        let name = OsString::from(format!("{QUEUE_PREFIX}{random:016x}"));
        let mut writing = name.clone();
        writing.push(QUEUE_WRITING);
        let mut file = match folder.create_hidden(&writing) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last = error;
                continue;
            }
            Err(error) => return Err(error),
        };
        let renamed = std::io::Write::write_all(&mut file, bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| folder.rename_new(&file, &writing, &name));
        if renamed.is_err() {
            drop(folder.remove_plain(&writing));
        }
        return renamed;
    }
    Err(last)
}

/// The notes queued in `cwd`: each `.clauseo-untold-<16 hex digits>` file there is read
/// (refusing a link), removed through the folder's handle, and only then its notes taken, so
/// two guards never tell one twice. A file that does not read as a list of notes is removed.
fn take_queued(cwd: &Path) -> Vec<String> {
    let Ok(folder) = write::Folder::open(cwd) else {
        return Vec::new();
    };
    let mut notes = Vec::new();
    for name in folder.names().into_iter().filter(|name| is_queued(name)) {
        let read = write::read_no_follow(&cwd.join(&name));
        if folder.remove_plain(&name).is_err() {
            continue;
        }
        if let Some(queued) = read
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
        {
            notes.extend(queued);
        }
    }
    notes
}

/// Whether `name` is a queued notes file's: [`QUEUE_PREFIX`] and 16 lowercase hex digits.
fn is_queued(name: &OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(QUEUE_PREFIX))
        .is_some_and(|random| {
            random.len() == 16
                && random
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

/// The note for a file delivered as `name`.
pub(crate) fn note(name: &str) -> String {
    format!("Saved {name} to downloads/ in the matter.")
}

/// What comes right before the path in [`fallback_note`]; the host takes the text after the
/// last one as the path (`crates/host`, the browser's tool results), and trusts it only inside
/// the matter's own fallback folder.
pub(crate) const FALLBACK_MARKER: &str = " Its path: ";

/// The note for a file delivered outside the matter, as `name` at `path`: one line, the path
/// last, after [`FALLBACK_MARKER`].
pub(crate) fn fallback_note(name: &str, path: &Path) -> String {
    format!(
        "Saved {name} outside the matter, in Clauseo's own folder for this matter: the matter's \
         drive cannot keep the \"downloaded from the internet\" mark, so the file is not in \
         downloads/. Your user sees where it is in the thread.{FALLBACK_MARKER}{}",
        path.display()
    )
}

/// For `line`, agent-browser's reply to an `agent_browser_open` call: when it failed with
/// `net::ERR_ABORTED` (a navigation that turned into a download), a success reply to the same
/// id instead, once `saved` has a note (each told in it), or, at `wait`'s end, saying the
/// download is still in progress while `staging` holds a file. `None` for any other reply, and
/// when no download shows by then: the error then goes on as read.
pub(crate) fn open_reply(
    line: &[u8],
    saved: &Saved,
    staging: &Path,
    wait: Duration,
) -> Option<Vec<u8>> {
    let reply: Value = serde_json::from_slice(line).ok()?;
    if !aborted(&reply) {
        return None;
    }
    // Counted in polls: the guard's threads have no clock (`browser_guard::pause`).
    let mut polls = wait.as_millis() / OPEN_POLL.as_millis();
    let texts = loop {
        let told = saved.take();
        if !told.is_empty() {
            break told;
        }
        if polls == 0 {
            if !in_staging(staging) {
                return None;
            }
            break vec![IN_PROGRESS.to_owned()];
        }
        polls -= 1;
        pause(OPEN_POLL);
    };
    let content: Vec<Value> = texts
        .into_iter()
        .map(|text| serde_json::json!({"type": "text", "text": text}))
        .collect();
    let answer = serde_json::json!({
        "jsonrpc": "2.0",
        "id": reply.get("id").cloned().unwrap_or(Value::Null),
        "result": {"content": content},
    });
    serde_json::to_vec(&answer).ok()
}

/// Whether `reply` is a tool result marked `isError` (or a JSON-RPC error) whose text names
/// [`ABORTED`].
fn aborted(reply: &Value) -> bool {
    if let Some(error) = reply.get("error") {
        return error.to_string().contains(ABORTED);
    }
    let Some(result) = reply.get("result") else {
        return false;
    };
    result.get("isError").and_then(Value::as_bool) == Some(true)
        && result.to_string().contains(ABORTED)
}

/// Whether `staging` holds anything but the mark probe: a download being written, or a
/// finished one the mover has not delivered yet.
fn in_staging(staging: &Path) -> bool {
    std::fs::read_dir(staging).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name() != std::ffi::OsStr::new(super::marks::PROBE))
    })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
