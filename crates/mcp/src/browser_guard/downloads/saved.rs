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

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

use crate::browser_guard::pause;

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
}

impl Saved {
    /// One more file delivered, under `name`, in the downloads folder.
    pub(crate) fn record(&self, name: &str) {
        lock(&self.untold).push(note(name));
    }

    /// One more file that stays in staging for good, with `why`.
    pub(crate) fn record_unsaved(&self, why: String) {
        lock(&self.untold).push(why);
    }

    /// The notes untold so far, which are told now.
    pub(crate) fn take(&self) -> Vec<String> {
        std::mem::take(&mut *lock(&self.untold))
    }

    /// `line`, a reply to a tool call, with one text block per untold note appended to its
    /// `result.content` (made when the result has none); they are told then. `None` (nothing
    /// told) when none is untold, or the line has no result object (an error reply) or a
    /// `content` that is no list: the notes wait for the next result.
    pub(crate) fn note_into(&self, line: &[u8]) -> Option<Vec<u8>> {
        let mut untold = lock(&self.untold);
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

/// The note for a file delivered as `name`.
pub(crate) fn note(name: &str) -> String {
    format!("Saved {name} to downloads/ in the matter.")
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
