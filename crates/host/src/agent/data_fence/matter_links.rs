//! Whether the matter folder may get its read allow rules (`own_allow.rs`
//! `matter_shell_allow_rules`): not when a link in it leads outside it. Claude Code 2.1.293's
//! path check reads only the folder before a glob (`cat <matter>/*/x`, `ls <matter>/li?k/`),
//! so through such a link a glob would read what the link leads to, `<data>` or another matter,
//! with no classifier (the security review of #1065, H2). The walk here never follows a link and
//! finds every symbolic link, junction and folder symbolic link (std's `is_symlink`, a
//! name-surrogate reparse point on Windows) whose target, resolved, is not in the matter; a
//! broken one counts as outside. On Windows any other reparse point counts as a link out too,
//! unless its tag is a known local kind of data (`network_paths::local_reparse_tag`:
//! `OneDrive`'s cloud files, dedup, WOF, WCI, an app execution alias, `ProjFS`, an `AF_UNIX`
//! socket; `counts_as_link`), so a matter in `OneDrive` keeps its rules while one holding a DFS
//! or Azure File Sync point does not. A `.lnk` shortcut is a plain file to the file system and
//! to Claude Code's tools: not a link.
//!
//! Another matter inside this one (a `.clauseo` marker folder below the matter's own) gives no
//! rules either, however deep and however many entries come before it: #1085 denies reading a
//! matter the conversation is not invited into, its fence lists only so many paths, and a glob
//! from this matter's folder would reach past that deny (`LaunchDeny::denies_inside` asks the
//! same for the matters the fence did list).
//!
//! The walk fails closed: a matter that can't be resolved or listed, an entry that can't be read
//! or whose kind can't be read, a walk past [`SCAN_CAP`] entries or past [`WALK_BUDGET`] all
//! give no rules, and the classifier judges, as before. Only a sub-folder that can't be listed at
//! all is skipped, as `clauseo_sandbox::scan_matter` does: the agent runs as the same user and
//! can't list it either. It runs off the actor, at the launch (`launch_checks.rs`, before the
//! preparation) and again at each turn's start on a running agent (`actor/turn/matter_walk.rs`):
//! a link that appeared since (an unpacked zip, a sync client) stops that agent, and the agent
//! that replaces it resumes the session with no matter rules.
//!
//! The residual: a link made during one turn, after its walk. Making one is a write (`ln -s`,
//! `mklink /J`, `New-Item -ItemType Junction`) that no allow rule matches, so it needs a
//! classifier's pass first, or another program makes it; on macOS the sandbox's `denyRead` of
//! `<data>` and the other matters still holds behind it. A hard link to a file outside is not
//! found here, and the Read tool reads one the same way.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::MatterId;

use crate::agent::codex::prepare::{Job, off_actor};
use crate::clock::Clock;
use crate::log_file::Cause;

/// The most entries the walk lists before it gives up (and the matter gets no rules).
pub(crate) const SCAN_CAP: usize = 50_000;

/// The longest a walk may take, at a launch or a turn's start, before the matter gets no rules:
/// a matter of tens of thousands of entries on a mapped drive, or a `OneDrive` folder whose
/// files are not downloaded yet, must not hold every launch and turn.
pub(crate) const WALK_BUDGET: Duration = Duration::from_secs(2);

/// What one entry of a listed folder is, to the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A symbolic link, junction or folder symbolic link: resolved to see where it leads.
    Link,
    /// Another reparse point that counts as a link out ([`counts_as_link`]); Windows only.
    Reparse,
    /// A folder: walked.
    Folder,
    /// Anything else.
    File,
}

/// An entry of a listed folder: its path, and its kind, which can fail to be read.
#[derive(Debug)]
pub(crate) struct Found {
    pub(crate) path: PathBuf,
    pub(crate) kind: io::Result<Kind>,
}

/// The entries of one listed folder, each of which can fail to be read.
pub(crate) type Listing<'a> = Box<dyn Iterator<Item = io::Result<Found>> + 'a>;

/// What the walk reads: the disk ([`Disk`]), or a test's stand-in.
pub(crate) trait WalkFs {
    /// `path` resolved through every link (`dunce::canonicalize`).
    fn resolve(&self, path: &Path) -> io::Result<PathBuf>;
    /// The entries of `folder`.
    fn list(&self, folder: &Path) -> io::Result<Listing<'_>>;
}

/// The file system itself.
pub(crate) struct Disk;

impl WalkFs for Disk {
    fn resolve(&self, path: &Path) -> io::Result<PathBuf> {
        dunce::canonicalize(path)
    }

    fn list(&self, folder: &Path) -> io::Result<Listing<'_>> {
        let entries = std::fs::read_dir(folder)?;
        Ok(Box::new(entries.map(|entry| {
            entry.map(|entry| {
                let path = entry.path();
                let kind = entry.file_type().map(|kind| {
                    if kind.is_symlink() {
                        Kind::Link
                    } else if unknown_reparse_point(&entry, &path) {
                        Kind::Reparse
                    } else if kind.is_dir() {
                        Kind::Folder
                    } else {
                        Kind::File
                    }
                });
                Found { path, kind }
            })
        })))
    }
}

/// The matter's spellings that may get read allow rules: `real` (the folder Claude works in)
/// and `stored` when it differs, or none when the walk of `real` finds a link out of it, passes
/// [`SCAN_CAP`] or can't list it. With no time limit: the launch and a turn's start take
/// [`readable_matter_within`]; the tests' form.
#[cfg(test)]
pub(crate) fn readable_matter(real: &Path, stored: &Path) -> Vec<PathBuf> {
    readable_in(&Disk, real, stored, &|| false)
}

/// [`readable_matter`] of matter folder `stored` and the real path Claude works in
/// (`agent::session_folder`), off the actor (`codex::prepare::off_actor`) and within
/// [`WALK_BUDGET`] on `clock`: none when it runs out. The walk stops itself at the budget, and
/// the wait gives up then too, for a listing the disk never answers (its thread ends when the
/// disk does).
pub(crate) async fn readable_matter_within(
    clock: Arc<dyn Clock>,
    matter: MatterId,
    stored: PathBuf,
) -> Vec<PathBuf> {
    walk_within(clock, matter, Disk, stored).await
}

/// [`readable_matter_within`] on `fs`.
pub(crate) async fn walk_within(
    clock: Arc<dyn Clock>,
    matter: MatterId,
    fs: impl WalkFs + Send + 'static,
    stored: PathBuf,
) -> Vec<PathBuf> {
    let walk_clock = Arc::clone(&clock);
    let start = clock.now().mono;
    let expired = move || walk_clock.now().mono.saturating_sub(start) >= WALK_BUDGET;
    let job = Job {
        what: "matter link walk",
        matter: Some(matter),
        wait: WALK_BUDGET,
    };
    let walked = off_actor(clock, job, move || {
        let real = crate::agent::session_folder(&stored);
        Ok(readable_in(&fs, &real, &stored, &expired))
    });
    walked.await.unwrap_or_else(|error| {
        tracing::info!(
            error = %Cause(&error),
            %matter,
            "the matter's link walk did not answer in time; its read allow rules were left out"
        );
        Vec::new()
    })
}

/// [`readable_matter`] on `fs`, giving up once `expired` says so.
fn readable_in(
    fs: &dyn WalkFs,
    real: &Path,
    stored: &Path,
    expired: &dyn Fn() -> bool,
) -> Vec<PathBuf> {
    match walk(fs, real, SCAN_CAP, expired) {
        Some(false) => std::iter::once(real.to_owned())
            .chain((real != stored).then(|| stored.to_owned()))
            .collect(),
        found => {
            let reason = if found.is_some() {
                "a link leads out of it, or another matter lies in it"
            } else {
                "it could not be walked within the cap and the time limit"
            };
            tracing::info!(reason, "the matter's read allow rules were left out");
            Vec::new()
        }
    }
}

/// Whether a link in `matter` leads outside it or another matter lies in it (`Some(true)`),
/// neither (`Some(false)`), or
/// the walk could not tell: `matter` can't be resolved or listed, or it holds more than `cap`
/// entries (`None`). With no time limit; the tests' form.
#[cfg(test)]
pub(crate) fn links_out(matter: &Path, cap: usize) -> Option<bool> {
    walk(&Disk, matter, cap, &|| false)
}

/// [`links_out`] on `fs`, and `None` once `expired` says so. An entry whose listing or kind
/// fails also gives `None`: what the walk could not see may be a link. A sub-folder that can't
/// be listed at all is skipped, as `clauseo_sandbox::scan_matter` does.
pub(crate) fn walk(
    fs: &dyn WalkFs,
    matter: &Path,
    cap: usize,
    expired: &dyn Fn() -> bool,
) -> Option<bool> {
    let root = fs.resolve(matter).ok()?;
    let mut folders = vec![root.clone()];
    let mut seen = 0_usize;
    let mut first = true;
    while let Some(folder) = folders.pop() {
        if expired() {
            return None;
        }
        let entries = match fs.list(&folder) {
            Ok(entries) => entries,
            Err(_) if first => return None,
            Err(error) => {
                tracing::debug!(%error, "the matter's link walk skipped a folder");
                continue;
            }
        };
        first = false;
        for entry in entries {
            seen += 1;
            if seen > cap || expired() {
                return None;
            }
            let Found { path, kind } = entry.ok()?;
            match kind.ok()? {
                Kind::Reparse => return Some(true),
                Kind::Link => {
                    let inside = fs
                        .resolve(&path)
                        .is_ok_and(|target| target.starts_with(&root));
                    if !inside {
                        return Some(true);
                    }
                }
                Kind::Folder if folder != root && is_marker(&path) => return Some(true),
                Kind::Folder => folders.push(path),
                Kind::File => {}
            }
        }
    }
    Some(false)
}

/// Whether `path` is named as a matter's marker folder (`.clauseo`, any letter case).
fn is_marker(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(crate::agent::loads::sandbox::MATTER_MARKER))
}

/// Whether a reparse point with `tag` counts as a link that may lead out of the matter: a
/// symbolic link, a junction or mount point, or any kind not known to be local data. Pure, so
/// it is tested on every system.
#[cfg(any(windows, test))]
fn counts_as_link(tag: u32) -> bool {
    !crate::network_paths::local_reparse_tag(tag)
}

/// Whether `entry` (at `path`, not a name-surrogate link) is a reparse point that
/// [`counts_as_link`]; one whose tag can't be read counts too. Never on other systems.
#[cfg(windows)]
fn unknown_reparse_point(entry: &std::fs::DirEntry, path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    /// `FILE_ATTRIBUTE_REPARSE_POINT`.
    const REPARSE_POINT: u32 = 0x400;
    let Ok(meta) = entry.metadata() else {
        return true;
    };
    meta.file_attributes() & REPARSE_POINT != 0
        && crate::network_paths::reparse_tag(path).is_none_or(counts_as_link)
}

/// Never on systems without reparse points.
#[cfg(not(windows))]
fn unknown_reparse_point(_entry: &std::fs::DirEntry, _path: &Path) -> bool {
    false
}

#[cfg(test)]
#[path = "matter_links_tests.rs"]
mod tests;
