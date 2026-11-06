//! Chrome's own downloads, moved from the profile's staging folder into `<cwd>/downloads` under
//! a name that is never one of the names that would plant agent instructions or config.
//!
//! A web page picks a download's name (`Content-Disposition`, `<a download>`), so a page could
//! save `CLAUDE.md` or `.mcp.json`. Chrome therefore downloads into the staging folder in its
//! profile (`chrome.rs`), and a thread here looks every [`POLL`] for finished files (Chrome
//! writes `*.crdownload` and renames to the final name only when done) and moves each into the
//! downloads folder: a protected name becomes `<stem> (download)<ext>`, any taken name gets
//! ` (2)`, ` (3)` and so on. Each move never replaces a file (`write::move_new`): a file that
//! appears under the chosen name between the look and the move makes the mover take the next
//! free name instead. A protected name therefore never exists in the downloads folder;
//! it can exist in the staging folder, inside the session's browser state folder (outside the
//! matter and the work folder), for up to one poll.
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
        // A refusal (Windows: Chrome still holds the file) is retried on the next sweep.
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

/// Chrome's names for a download still being written.
fn in_progress(name: &str) -> bool {
    name.ends_with(".crdownload")
        || name.starts_with(".com.google.Chrome.")
        || name.starts_with(".org.chromium.Chromium.")
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
mod tests {
    use std::path::Path;

    use super::{is_protected, place, safe_name};
    use crate::write;

    /// A file that appears under the chosen name between the mover's look and its move is kept,
    /// and the download takes the next free name; so does one that appears there too.
    #[test]
    fn a_file_that_appears_at_the_target_is_not_replaced() {
        let dir = std::env::temp_dir().join(format!("clauseo-mcp-place-{}", std::process::id()));
        if dir.exists() {
            write::remove_dir_all(&dir).unwrap();
        }
        let (staging, downloads) = (dir.join("staging"), dir.join("downloads"));
        write::create_dir_all(&staging).unwrap();
        write::create_dir_all(&downloads).unwrap();
        let from = staging.join("report.pdf");
        write::write(&from, "download").unwrap();
        let mut raced = 0;
        let placed = place(&from, &downloads, "report.pdf", |target: &Path| {
            if raced < 2 {
                raced += 1;
                write::write(target, "the agent's own").unwrap();
            }
        })
        .unwrap();
        assert_eq!(placed, downloads.join("report (3).pdf"));
        let read = |name: &str| std::fs::read_to_string(downloads.join(name)).unwrap();
        assert_eq!(read("report.pdf"), "the agent's own");
        assert_eq!(read("report (2).pdf"), "the agent's own");
        assert_eq!(read("report (3).pdf"), "download");
        assert!(!from.exists());
        write::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn protected_names_are_renamed_in_any_case() {
        for (name, safe) in [
            ("CLAUDE.md", "CLAUDE (download).md"),
            ("claude.MD", "claude (download).MD"),
            (".mcp.json", ".mcp (download).json"),
            ("AGENTS.md", "AGENTS (download).md"),
            ("agents.md. ", "agents (download).md"),
            (".claude", ".claude (download)"),
            (".Codex", ".Codex (download)"),
            ("CLAUDE.local.md", "CLAUDE.local (download).md"),
            ("AGENT\u{17f}.md", "AGENT\u{17f} (download).md"),
        ] {
            assert!(is_protected(name), "{name}");
            assert_eq!(safe_name(name), safe);
            assert!(!is_protected(&safe_name(name)), "{safe}");
        }
    }

    #[test]
    fn other_names_are_kept() {
        for name in [
            "report.pdf",
            "CLAUDE.md.txt",
            "my CLAUDE.md",
            ".mcp",
            "x.crdownload",
        ] {
            assert!(!is_protected(name), "{name}");
            assert_eq!(safe_name(name), name);
        }
    }
}
