//! Downloads Chrome holds for a confirmation that never comes, which the guard never delivers
//! (decision record 0029, "Executable downloads"). Chrome gives a download it rates dangerous
//! (on Windows an `.exe`, with Safe Browsing off) the name `<prefix> <n>.crdownload`, `<prefix>`
//! being `IDS_DOWNLOAD_UNCONFIRMED_PREFIX` in Chrome's language ("Unconfirmed" in English) and
//! `<n>` a number from 0 to 1 000 000 (`download_target_determiner.cc`,
//! `kUnconfirmedFormatSuffix`), and keeps it under that name until someone clicks Keep. Headless,
//! nobody can, so the file would stay in staging and the agent would wait for it in vain.
//!
//! The mover records each such file once, when it has stayed under that name for
//! [`HELD_SWEEPS`] sweeps in a row ([`Held::record`]); the next tool call's result the
//! guard relays gets one more text block ([`Held::note_into`]) telling the agent the download
//! was not delivered. When Chrome stops, the held files are deleted from staging ([`clear`]).
//! Their names carry nothing of the download's own, so the note cannot name it.
//!
//! A download still in progress whose own name has no extension and ends in a space and a number
//! (`Report 12`, a name a page can choose) looks the same while it is written. One that finishes
//! within a sweep of being seen is never counted; one still being written at its second sweep
//! is, so the agent hears it was held although it lands, and a stop before it finishes deletes it.

use std::io;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;

use crate::browser_guard::pause;
use crate::write;

/// How many sweeps in a row a held name must be seen before it is recorded: a download that
/// Chrome renames by the next sweep was never held (a page's `Report 12` that finished at once).
pub(crate) const HELD_SWEEPS: u32 = 2;
/// The most digits of Chrome's number (`kUnconfirmedUniquifierRange`, 1 000 000).
const MAX_DIGITS: usize = 7;
/// How many times [`clear`] tries a held file Windows will not delete yet, [`CLEAR_PAUSE`] apart.
const CLEAR_TRIES: usize = 10;
const CLEAR_PAUSE: Duration = Duration::from_millis(100);

/// Whether `name` is Chrome's name for a download held for confirmation:
/// `<prefix> <digits>.crdownload`, with a prefix that holds no dot.
pub(crate) fn is_held(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".crdownload") else {
        return false;
    };
    let Some((prefix, number)) = stem.rsplit_once(' ') else {
        return false;
    };
    !prefix.trim().is_empty()
        && !prefix.contains('.')
        && (1..=MAX_DIGITS).contains(&number.len())
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

/// The held downloads the agent has not been told about yet.
#[derive(Default)]
pub(crate) struct Held {
    untold: Mutex<usize>,
}

impl Held {
    /// One more held download.
    pub(crate) fn record(&self) {
        *lock(&self.untold) += 1;
    }

    /// `line`, a reply to a tool call, with the note appended to its `result.content` (made when
    /// the result has none) when held downloads are untold; they are told then. `None` (nothing
    /// told) when none is, or the line has no result object (an error reply) or a `content` that
    /// is no list: the note waits for the next result. The reply's own content is kept as it is;
    /// only the note is added after it.
    pub(crate) fn note_into(&self, line: &[u8]) -> Option<Vec<u8>> {
        let mut untold = lock(&self.untold);
        if *untold == 0 {
            return None;
        }
        let mut reply: Value = serde_json::from_slice(line).ok()?;
        let content = reply
            .get_mut("result")?
            .as_object_mut()?
            .entry("content")
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()?;
        content.push(serde_json::json!({"type": "text", "text": note(*untold)}));
        let bytes = serde_json::to_vec(&reply).ok()?;
        *untold = 0;
        Some(bytes)
    }
}

/// The note for `count` held downloads.
pub(crate) fn note(count: usize) -> String {
    let what = if count == 1 {
        "1 download".to_owned()
    } else {
        format!("{count} downloads")
    };
    format!(
        "Clauseo does not deliver executable downloads. The browser held back {what} as an \
         executable or another file type it treats as dangerous, so it was not saved to the \
         downloads folder and will be deleted. Tell your user it was not delivered."
    )
}

/// Deletes every held download in `staging`, once Chrome has stopped: none can finish now. A
/// file Windows will not delete yet (Chrome's last handle closing) is tried again, up to
/// [`CLEAR_TRIES`] times; one left after that goes with the profile at the guard's exit.
pub(crate) fn clear(staging: &Path) {
    for attempt in 0..CLEAR_TRIES {
        let Ok(entries) = std::fs::read_dir(staging) else {
            return;
        };
        let mut left = false;
        for entry in entries.flatten() {
            if !is_held(&entry.file_name().to_string_lossy()) {
                continue;
            }
            let path = entry.path();
            if !std::fs::symlink_metadata(&path).is_ok_and(|found| found.is_file()) {
                continue;
            }
            match write::remove_file(&path) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => left = true,
                _ => {}
            }
        }
        if !left {
            return;
        }
        if attempt + 1 < CLEAR_TRIES {
            pause(CLEAR_PAUSE);
        }
    }
    crate::browser_guard::note("a download held for confirmation could not be deleted yet");
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
