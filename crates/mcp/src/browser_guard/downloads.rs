//! Chrome's own downloads, moved from the profile's staging folder into the downloads folder
//! (the guard's `--downloads`: `<matter>/downloads` in a Codex session, else `<cwd>/downloads`)
//! under a name that is never one of the names that would plant agent instructions or config.
//! Each delivered file is told to the agent in its next tool result (`saved.rs`).
//!
//! A web page picks a download's name (`Content-Disposition`, `<a download>`), so a page could
//! save `CLAUDE.md` or `.mcp.json`. Chrome therefore downloads into the staging folder in its
//! profile (`chrome.rs`), and a thread here looks every [`POLL`] for finished files (Chrome
//! writes `*.crdownload`, or on Windows first `<guid>.tmp`, and renames to the final name only
//! when done: [`in_progress`]) and moves each into the downloads folder: a protected name
//! becomes `<stem> (download)<ext>`, any taken name gets ` (2)`, ` (3)` and so on. Each move
//! never replaces a file (`write::Folder::move_new`): a file that appears under the chosen name
//! between the look and the move makes the mover take the next free name instead. A protected name
//! therefore never exists in the downloads folder; it can exist in the staging folder, inside
//! the session's browser state folder (outside the matter and the work folder), for up to one
//! poll, or up to [`MARK_WAIT`] polls on Windows and macOS.
//!
//! The downloads folder sits in the matter, which the agent's sandboxed shell can write, while
//! the guard runs outside the sandbox: the agent could swap the folder for a link to anywhere
//! (`~/Library/LaunchAgents`) while a page serves a file. Each move therefore opens the folder
//! once, refusing a link at its name, and finds the name and moves the file relative to that
//! handle, never through the folder's path again (`write::Folder`; on Windows, where the guard
//! does not run yet, still by path).
//!
//! Each moved file has its OS download mark, so the OS checks it when the lawyer opens it
//! (`marks.rs`): the mover waits for Chrome's mark, makes sure of it in staging, and only then
//! moves the file. Where the drive cannot hold a mark, downloads are held back ([`Sweeper`]).
//!
//! A download Chrome holds for a confirmation (an executable, `held.rs`) is never delivered: the
//! agent is told in its next tool result, and the file is deleted from staging when Chrome stops.
//!
//! Why not Chrome's `Browser.setDownloadBehavior` with `allowAndName`, which saves files by
//! GUID so the page's name never touches the disk: the behaviour is one per browser context,
//! the last client to set it wins, and agent-browser sets `default` each time it connects
//! (v0.41.1 `browser.rs:784`, `browser.rs:2563-2574`); Chrome also resets it when the client
//! that set it disconnects. The guard would have to win that race on every connect.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::write;

mod held;
mod marks;
mod saved;

pub(crate) use held::Held;
pub(crate) use saved::{OPEN_WAIT, Saved, open_reply};

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
/// How many polls a finished file without its OS mark waits for Chrome's (30 s): Chrome renames
/// a download to its final name first and marks it after (`download_file_impl.cc`
/// `RenameWithRetryInternal`), on Windows through Attachment Services' scan, which "has been
/// known to take longer than 10 seconds" (`quarantine_win.cc`). A move before that would make
/// Chrome fail the download and let out a file the scan would have deleted. Each file waits on
/// its own, so a slow one holds back no other.
pub(crate) const MARK_WAIT: u32 = 150;
/// The note left in the downloads folder when downloads are held back.
pub(crate) const BLOCKED_NOTE: &str = "DOWNLOADS-BLOCKED.txt";
/// Its text.
const BLOCKED_TEXT: &str = "Downloads from the browser are held back.\n\nThis folder's drive \
    cannot record the \"downloaded from the internet\" mark that Windows and macOS use to check \
    downloaded files before they open, so the browser's downloads are not put here. They are \
    deleted when the session ends. A work folder on a drive that keeps the mark (NTFS on \
    Windows, APFS on macOS) lets downloads through.\n";

/// The thread that moves finished downloads.
pub(crate) struct Mover {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<Sweeper>>,
    staging: PathBuf,
    downloads: PathBuf,
}

impl Mover {
    /// Starts moving from `staging` to `downloads` every [`POLL`], recording each held download
    /// in `held` and each delivered file in `saved`.
    pub(crate) fn start(
        staging: PathBuf,
        downloads: PathBuf,
        held: Arc<Held>,
        saved: Arc<Saved>,
    ) -> Mover {
        let (stop, stopped) = mpsc::channel::<()>();
        let mut sweeper = Sweeper::new(staging.clone(), downloads.clone(), MARK_WAIT);
        sweeper.held = Some(held);
        sweeper.saved = Some(saved);
        let thread = thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(POLL) {
                sweeper.sweep();
            }
            sweeper
        });
        Mover {
            stop: Some(stop),
            thread: Some(thread),
            staging,
            downloads,
        }
    }

    /// Stops the thread, then moves what finished since its last look, waiting no more for a
    /// mark, and deletes the held downloads: Chrome has stopped.
    pub(crate) fn finish(mut self) {
        drop(self.stop.take());
        let joined = self.thread.take().map(JoinHandle::join);
        let mut sweeper = match joined {
            Some(Ok(sweeper)) => sweeper,
            Some(Err(_)) => {
                super::note("the download mover panicked");
                Sweeper::new(self.staging.clone(), self.downloads.clone(), 0)
            }
            None => Sweeper::new(self.staging.clone(), self.downloads.clone(), 0),
        };
        sweeper.wait = 0;
        sweeper.sweep();
        held::clear(&self.staging);
    }
}

/// One sweep after Chrome has stopped (the reaper's, on Unix): [`Sweeper::sweep`] with no wait,
/// then the held downloads deleted.
#[cfg(any(unix, test))]
pub(crate) fn sweep(staging: &Path, downloads: &Path) {
    Sweeper::new(staging.to_owned(), downloads.to_owned(), 0).sweep();
    held::clear(staging);
}

/// The mover's state from one sweep to the next.
pub(crate) struct Sweeper {
    staging: PathBuf,
    downloads: PathBuf,
    /// How many sweeps a finished file without Chrome's mark waits for it.
    wait: u32,
    /// Finished files seen without Chrome's mark, with how many sweeps each has waited.
    waited: HashMap<PathBuf, u32>,
    /// Files whose mark could not be written, or that cannot move to another drive, each noted
    /// once.
    unmarkable: HashSet<PathBuf>,
    /// Whether a mark can be written in the staging folder: probed at the first finished file.
    marks_work: Option<bool>,
    /// The probe ([`marks::probe`]; the tests inject a failure).
    probe: fn(&Path) -> io::Result<()>,
    /// Where each held download is recorded, once (the reaper's sweep has none).
    held: Option<Arc<Held>>,
    /// The held downloads seen, with how many sweeps in a row each was seen.
    held_seen: HashMap<PathBuf, u32>,
    /// Where each delivered file is recorded (the reaper's sweep has none).
    saved: Option<Arc<Saved>>,
}

impl Sweeper {
    pub(crate) fn new(staging: PathBuf, downloads: PathBuf, wait: u32) -> Sweeper {
        Sweeper::with_probe(staging, downloads, wait, marks::probe)
    }

    pub(crate) fn with_probe(
        staging: PathBuf,
        downloads: PathBuf,
        wait: u32,
        probe: fn(&Path) -> io::Result<()>,
    ) -> Sweeper {
        Sweeper {
            staging,
            downloads,
            wait,
            waited: HashMap::new(),
            unmarkable: HashSet::new(),
            marks_work: None,
            probe,
            held: None,
            held_seen: HashMap::new(),
            saved: None,
        }
    }

    /// Moves every finished regular file in the staging folder into the downloads folder under
    /// [`safe_name`] (made unique), each with its OS mark, written in staging, out of the agent's
    /// reach, before the move, which keeps it. A file without Chrome's mark first waits
    /// [`Sweeper::wait`] sweeps. Where no mark can be written (Windows and macOS), nothing is
    /// moved: the files stay in staging, deleted with the profile, and the downloads folder
    /// gets [`BLOCKED_NOTE`]. A file that cannot be moved now is left for the next sweep. A file
    /// Chrome holds for confirmation ([`held::is_held`]) is recorded once, when it is still there
    /// at its [`held::HELD_SWEEPS`]th sweep in a row, and never moved.
    pub(crate) fn sweep(&mut self) {
        let Ok(entries) = std::fs::read_dir(&self.staging) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if held::is_held(&name) {
                let sweeps = self.held_seen.entry(entry.path()).or_insert(0);
                *sweeps = sweeps.saturating_add(1);
                if *sweeps == held::HELD_SWEEPS
                    && let Some(held) = &self.held
                {
                    held.record();
                }
                continue;
            }
            if in_progress(&name) || name == marks::PROBE {
                continue;
            }
            let path = entry.path();
            if !std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file())
                || !self.marks_work()
            {
                continue;
            }
            if !marks::marked(&path) {
                let waited = self.waited.entry(path.clone()).or_insert(0);
                if *waited < self.wait {
                    *waited += 1;
                    continue;
                }
            }
            if let Err(error) = marks::ensure(&path) {
                if self.unmarkable.insert(path.clone()) {
                    super::note(&format!(
                        "a download stays in staging: its \"from the internet\" mark could not \
                         be written: {error}"
                    ));
                }
                continue;
            }
            // A refusal (Windows: a sharing violation while Chrome, or a scanner, still holds the
            // file without delete sharing) is retried on the next sweep: the file stays in staging.
            match place(&path, &self.downloads, &safe_name(&name), |_| {}) {
                Ok(placed) => {
                    self.waited.remove(&path);
                    self.unmarkable.remove(&path);
                    if let (Some(saved), Some(given)) = (&self.saved, placed.file_name()) {
                        saved.record(&given.to_string_lossy());
                    }
                }
                // A matter on another drive than the data folder: the move, which keeps the
                // mark, cannot happen, ever. Told once; the file goes with the profile.
                Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
                    if self.unmarkable.insert(path.clone()) {
                        super::note(
                            "a download stays in staging: the downloads folder is on another drive",
                        );
                        if let Some(saved) = &self.saved {
                            saved.record_unsaved(format!(
                                "The browser downloaded {}, but it is not in the matter: the \
                                 matter is on a different drive from Clauseo's data folder, so \
                                 the file stayed in Clauseo's browser folder, which is cleared \
                                 when the browser closes. Tell your user it was not saved to the \
                                 matter, and that they can download it from the page in their \
                                 own browser.",
                                safe_name(&name)
                            ));
                        }
                    }
                }
                Err(_) => {}
            }
        }
        let exists = |path: &PathBuf| std::fs::symlink_metadata(path).is_ok();
        self.waited.retain(|path, _| exists(path));
        self.unmarkable.retain(exists);
        self.held_seen.retain(|path, _| exists(path));
    }

    /// Whether marks can be written in the staging folder, probed once. When not, notes it and
    /// leaves [`BLOCKED_NOTE`] in the downloads folder (never replacing a file there).
    fn marks_work(&mut self) -> bool {
        if let Some(works) = self.marks_work {
            return works;
        }
        let works = match (self.probe)(&self.staging) {
            Ok(()) => true,
            Err(error) => {
                super::note(&format!(
                    "downloads are held back: the drive cannot record the \"downloaded from the \
                     internet\" mark ({error}); see {BLOCKED_NOTE} in the downloads folder"
                ));
                let made = write::Folder::open(&self.downloads)
                    .and_then(|folder| folder.create_new(OsStr::new(BLOCKED_NOTE)));
                if let Ok(mut file) = made {
                    drop(io::Write::write_all(&mut file, BLOCKED_TEXT.as_bytes()));
                }
                false
            }
        };
        self.marks_work = Some(works);
        works
    }
}

/// Moves the file `from` into `downloads` as `name`, or the first free `<stem> (<n>)<ext>`,
/// never replacing a file: when a name is taken by the time of the move, the next free one is
/// tried, [`MOVE_TRIES`] names at most. The folder is opened once, first ([`write::Folder`]):
/// a link at its name (or, on Windows, a reparse point) is refused there, and every look for a
/// free name and every move goes through that handle, so a link swapped in for `downloads`
/// afterwards is never followed: the move goes into the folder that was opened, or fails if it
/// is gone. `before_move` runs between the look and each move (the tests put a file there, or
/// swap the folder). Returns where the file went, by the path it had when opened.
pub(crate) fn place(
    from: &Path,
    downloads: &Path,
    name: &str,
    mut before_move: impl FnMut(&Path),
) -> io::Result<PathBuf> {
    let folder = write::Folder::open(downloads)?;
    let mut first = 1;
    for _ in 0..MOVE_TRIES {
        let Some((n, given)) = free_name(&folder, name, first) else {
            break;
        };
        let target = downloads.join(&given);
        before_move(&target);
        match folder.move_new(from, OsStr::new(&given)) {
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

/// The first of `name` (number 1), `<stem> (2)<ext>`, ... up to [`MAX_NUMBER`], from number
/// `first` on, that does not exist in `folder` now, with its number.
fn free_name(folder: &write::Folder, name: &str, first: u32) -> Option<(u32, String)> {
    let (stem, ext) = split_ext(name);
    (first..=MAX_NUMBER)
        .map(|n| {
            let given = if n == 1 {
                name.to_owned()
            } else {
                format!("{stem} ({n}){ext}")
            };
            (n, given)
        })
        .find(|(_, given)| !folder.has(OsStr::new(given)))
}

#[cfg(test)]
mod tests;
