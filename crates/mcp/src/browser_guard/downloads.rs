//! Chrome's own downloads, moved from the profile's staging folder into `<cwd>/downloads` under
//! a name that is never one of the names that would plant agent instructions or config.
//!
//! A web page picks a download's name (`Content-Disposition`, `<a download>`), so a page could
//! save `CLAUDE.md` or `.mcp.json`. Chrome therefore downloads into the staging folder in its
//! profile (`chrome.rs`), and a thread here looks every [`POLL`] for finished files (Chrome
//! writes `*.crdownload`, or on Windows first `<guid>.tmp`, and renames to the final name only
//! when done: [`in_progress`]) and moves each into the downloads folder: a protected name
//! becomes `<stem> (download)<ext>`, any taken name gets ` (2)`, ` (3)` and so on. Each move
//! never replaces a file (`write::move_new`): a file that appears under the chosen name between
//! the look and the move makes the mover take the next free name instead. A protected name
//! therefore never exists in the downloads folder; it can exist in the staging folder, inside
//! the session's browser state folder (outside the matter and the work folder), for up to one
//! poll.
//!
//! Why not Chrome's `Browser.setDownloadBehavior` with `allowAndName`, which saves files by
//! GUID so the page's name never touches the disk: the behaviour is one per browser context,
//! the last client to set it wins, and agent-browser sets `default` each time it connects
//! (v0.41.1 `browser.rs:784`, `browser.rs:2563-2574`); Chrome also resets it when the client
//! that set it disconnects. The guard would have to win that race on every connect.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::write;

/// Names no download may take, compared without ASCII case: the matter root's write-denied
/// names, `clauseo_sandbox::policy::PROTECTED_NAMES` (this crate may not depend on the sandbox;
/// `tests/guard/lifecycle.rs`'s `the_download_guard_protects_the_sandboxs_names` checks the two
/// lists match).
pub(crate) const PROTECTED_NAMES: [&str; 8] = [
    ".codex",
    ".claude",
    ".agents",
    ".clauseo",
    ".mcp.json",
    "CLAUDE.md",
    "CLAUDE.local.md",
    "AGENTS.md",
];

/// The highest ` (<n>)` the mover gives a taken name.
const MAX_NUMBER: u32 = 9999;
/// How many names one file's move tries when each is taken between the look and the move.
const MOVE_TRIES: usize = 16;

/// How often the staging folder is read.
pub(crate) const POLL: Duration = Duration::from_millis(200);

/// The thread that moves finished downloads.
pub(crate) struct Mover {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    staging: PathBuf,
    downloads: PathBuf,
}

impl Mover {
    /// Starts moving from `staging` to `downloads` every [`POLL`].
    pub(crate) fn start(staging: PathBuf, downloads: PathBuf) -> Mover {
        let (stop, stopped) = mpsc::channel::<()>();
        let (from, to) = (staging.clone(), downloads.clone());
        let thread = thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(POLL) {
                sweep(&from, &to);
            }
        });
        Mover {
            stop: Some(stop),
            thread: Some(thread),
            staging,
            downloads,
        }
    }

    /// Stops the thread, then moves what finished since its last look.
    pub(crate) fn finish(mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            super::note("the download mover panicked");
        }
        sweep(&self.staging, &self.downloads);
    }
}

/// Moves every finished regular file in `staging` into `downloads` under [`safe_name`] (made
/// unique). A file that cannot be moved now is left for the next sweep.
pub(crate) fn sweep(staging: &Path, downloads: &Path) {
    let Ok(entries) = std::fs::read_dir(staging) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if in_progress(&name) {
            continue;
        }
        let path = entry.path();
        if !std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
            continue;
        }
        // A refusal (Windows: a sharing violation while Chrome, or a scanner, still holds the
        // file without delete sharing) is retried on the next sweep: the file stays in staging.
        drop(place(&path, downloads, &safe_name(&name), |_| {}));
    }
}

/// Moves the file `from` into `downloads` as `name`, or the first free `<stem> (<n>)<ext>`,
/// never replacing a file: when a name is taken by the time of the move, the next free one is
/// tried, [`MOVE_TRIES`] names at most. `before_move` runs between the look and each move (the
/// tests put a file there). Returns where the file went.
pub(crate) fn place(
    from: &Path,
    downloads: &Path,
    name: &str,
    mut before_move: impl FnMut(&Path),
) -> io::Result<PathBuf> {
    let mut first = 1;
    for _ in 0..MOVE_TRIES {
        let Some((n, target)) = free_target(downloads, name, first) else {
            break;
        };
        before_move(&target);
        match write::move_new(from, &target) {
            Ok(()) => return Ok(target),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => first = n + 1,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free name in the downloads folder",
    ))
}

/// Chrome's names for a download still being written, on every OS: `*.crdownload` once Chrome
/// knows the download's name; before that, the file `base::CreateTemporaryFileInDir` makes
/// (`base_file.cc` `BaseFile::Initialize`): `.com.google.Chrome.XXXXXX` (or
/// `.org.chromium.Chromium.XXXXXX`) on Unix, and on Windows `<guid>.tmp`, a lowercase UUID
/// (`file_util_win.cc` `CreateAndOpenTemporaryFileInDirWithFlags`, `FormatTemporaryFileName`).
/// Any other `.tmp`, such as `notes.tmp`, is a finished download. A page that names its
/// download `<guid>.tmp` loses it: the file never moves and goes with the profile.
fn in_progress(name: &str) -> bool {
    name.ends_with(".crdownload")
        || name.starts_with(".com.google.Chrome.")
        || name.starts_with(".org.chromium.Chromium.")
        || name.strip_suffix(".tmp").is_some_and(is_guid)
}

/// Whether `text` is a UUID as `base::Uuid::AsLowercaseString` writes it: 8-4-4-4-12 hex digits
/// with hyphens. Upper case is accepted too: Windows names ignore case.
fn is_guid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Whether `name` is one of [`PROTECTED_NAMES`] by the rules of any of the guard's OSes: without
/// ASCII case, without the trailing dots and spaces Windows drops, and with `ſ` read as `s` (the
/// one letter outside ASCII that case folding, as macOS compares names, turns into a letter of
/// these names).
pub(crate) fn is_protected(name: &str) -> bool {
    let folded = name
        .trim_end_matches(['.', ' '])
        .replace('\u{17f}', "s")
        .to_ascii_lowercase();
    PROTECTED_NAMES
        .iter()
        .any(|protected| protected.to_ascii_lowercase() == folded)
}

/// `name`, or for a protected one `<stem> (download)<ext>`: `CLAUDE (download).md`,
/// `.mcp (download).json`, `.claude (download)`.
pub(crate) fn safe_name(name: &str) -> String {
    if !is_protected(name) {
        return name.to_owned();
    }
    let trimmed = name.trim_end_matches(['.', ' ']);
    let (stem, ext) = split_ext(trimmed);
    format!("{stem} (download){ext}")
}

/// `name` split before its last dot, a leading dot not counting: `("CLAUDE", ".md")`,
/// `(".mcp", ".json")`, `(".claude", "")`.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    }
}

/// The first of `dir/name` (number 1), `dir/<stem> (2)<ext>`, ... up to [`MAX_NUMBER`], from
/// number `first` on, that does not exist now, with its number.
fn free_target(dir: &Path, name: &str, first: u32) -> Option<(u32, PathBuf)> {
    let (stem, ext) = split_ext(name);
    (first..=MAX_NUMBER)
        .map(|n| {
            let path = if n == 1 {
                dir.join(name)
            } else {
                dir.join(format!("{stem} ({n}){ext}"))
            };
            (n, path)
        })
        .find(|(_, path)| std::fs::symlink_metadata(path).is_err())
}

#[cfg(test)]
mod tests;
