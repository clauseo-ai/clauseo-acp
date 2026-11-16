//! The agents' shell commands (`<data>/tools`, [`ProfilePaths::tools`]): the app copies the
//! bundled agent-browser sidecar there as `agent-browser`, so the command has the name its skill
//! and the instructions use, in a folder that holds nothing else for the agents' `PATH`. A copy,
//! never a link: a link would reach into the app's own folder, and the folder holds a program
//! only the app writes. The same way it copies the bundled `clauseo-mcp` to
//! `<data>/tools/no-git/git` ([`install_no_git`]), the `git` first on every agent's `PATH`.

use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use crate::error::ProfileError;
use crate::paths::ProfilePaths;

/// What [`install_agent_browser`] did, for the app's support line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    /// The copy already held the sidecar's bytes: nothing written.
    UpToDate,
    /// The copy was made or replaced.
    Copied,
    /// The copy was replaced while it ran (Windows): the running one was renamed aside first,
    /// for a later start's sweep to remove.
    CopiedAside,
}

impl Installed {
    /// Whether the copy was written.
    pub fn wrote(self) -> bool {
        self != Self::UpToDate
    }

    /// The word a log line shows.
    pub fn word(self) -> &'static str {
        match self {
            Self::UpToDate => "up_to_date",
            Self::Copied => "copied",
            Self::CopiedAside => "copied_running_copy_set_aside",
        }
    }
}

/// Copies `source` (the bundled sidecar) to [`ProfilePaths::agent_browser_command`], only when
/// the copy is missing or its SHA-256 differs from `source`'s, through a temp file beside it and
/// a rename, mode 0755 on Unix. Returns the copy's path and what it did ([`Installed`]). Reads
/// up to two programs (about 18 MB each) and may write one: at app start, off the host thread.
/// An error leaves the browser out; the caller logs it and goes on.
pub fn install_agent_browser(
    source: &Path,
    paths: &ProfilePaths,
) -> Result<(PathBuf, Installed), ProfileError> {
    let target = paths.agent_browser_command();
    let (installed, _) = install_copy(source, &target)?;
    Ok((target, installed))
}

/// Copies `source` (the bundled `clauseo-mcp`) to [`ProfilePaths::no_git_command`], the way
/// [`install_agent_browser`] copies its program: only when the copy is missing, is not a
/// regular file, or its SHA-256 differs. Returns what it did and `source`'s SHA-256, which the
/// host checks the copy against at every launch (`HostConfig::with_no_git`). Anything else in
/// the copy's folder is removed ([`sweep_beside`]). At app start,
/// off the host thread. An error leaves no sound copy; the caller logs it, and the host then
/// refuses every agent launch.
pub fn install_no_git(
    source: &Path,
    paths: &ProfilePaths,
) -> Result<(Installed, [u8; 32]), ProfileError> {
    let target = paths.no_git_command();
    let swept = sweep_beside(&target);
    let installed = install_copy(source, &target)?;
    swept.map(|()| installed)
}

/// Removes everything in `target`'s folder but `target` (its name compared ignoring case on
/// macOS and Windows, whose disks do) and names starting with `.` (the copy's own temp files
/// and, on Windows, a running copy set aside, which [`install_copy`]'s sweep removes once
/// nothing runs it; no lookup of `git` finds them): the host refuses a launch while the folder
/// holds anything else. Run before
/// the copy, so a case variant of the program's name is the program, not swept after it. A
/// folder there is removed whole and a read-only file made writable first; every entry is
/// tried, and the first failure comes back as the error.
fn sweep_beside(target: &Path) -> Result<(), ProfileError> {
    let (Some(folder), Some(name)) = (target.parent(), target.file_name()) else {
        return Ok(());
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        // No folder yet: the copy makes it.
        return Ok(());
    };
    let fold = cfg!(any(windows, target_os = "macos"));
    let mut failed = None;
    for entry in entries.flatten() {
        let other = entry.file_name();
        let same = if fold {
            other.eq_ignore_ascii_case(name)
        } else {
            other == name
        };
        if same || other.as_encoded_bytes().starts_with(b".") {
            continue;
        }
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            crate::write::remove_dir_all(&path);
        } else if let Err(err) = crate::write::remove_file_forced(&path) {
            failed = failed.or(Some(err));
        }
    }
    failed.map_or(Ok(()), Err)
}

/// Makes `target` a copy of the program `source`, through a temp file beside it and a
/// rename, mode 0755 on Unix, unless it is already a regular file holding the same bytes; old
/// copies set aside in its folder are swept first. Returns what it did and the SHA-256.
fn install_copy(source: &Path, target: &Path) -> Result<(Installed, [u8; 32]), ProfileError> {
    if let Some(folder) = target.parent() {
        crate::write::sweep_old(folder);
    }
    let bytes = std::fs::read(source).map_err(|err| ProfileError::io("read", source, err))?;
    let want: [u8; 32] = Sha256::digest(&bytes).into();
    let regular = std::fs::symlink_metadata(target).is_ok_and(|meta| meta.file_type().is_file());
    let held = std::fs::read(target)
        .ok()
        .filter(|_| regular)
        .map(|held| <[u8; 32]>::from(Sha256::digest(&held)));
    if held == Some(want) {
        return Ok((Installed::UpToDate, want));
    }
    let installed = if crate::write::replace_program(target, &bytes)? {
        Installed::CopiedAside
    } else {
        Installed::Copied
    };
    Ok((installed, want))
}

/// What [`remove_stale_browser`] removed, for the app's support line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StaleBrowser {
    /// Whether the `agent-browser` copy in `<data>/tools` was there and is gone.
    pub command: bool,
    /// How many files of the browser's skill, as the skills manifests record them, are gone.
    pub skill_files: usize,
}

/// Removes what an earlier start that copied agent-browser left (on Windows, before Claude lost
/// its browser): the copy at [`ProfilePaths::agent_browser_command`], any copy of it set aside
/// while it ran and any temp file a crashed copy left in [`ProfilePaths::tools`] (only those
/// Clauseo names), and the browser's skill folder in Claude's plugin and in Codex's skills,
/// each through its manifest (`skills::retire`), so only files Clauseo wrote go. For a start
/// where no agent gets the shell command (`servers::shell_browser`), so a stale skill never
/// reaches an agent. Each part is tried; the first failure comes back as the error (a copy still
/// running on Windows stays, for the next start). Nothing else under `<data>/tools` is touched.
pub fn remove_stale_browser(paths: &ProfilePaths) -> Result<StaleBrowser, ProfileError> {
    let mut removed = StaleBrowser::default();
    let mut failed = None;
    for root in [paths.claude_skills(), paths.codex_skills().to_path_buf()] {
        match crate::skills::retire(&root, &[crate::skills::builtin::AGENT_BROWSER]) {
            Ok(gone) => removed.skill_files += gone.len(),
            Err(err) => failed = failed.or(Some(err)),
        }
    }
    crate::write::sweep_old(paths.tools());
    let command = paths.agent_browser_command();
    if std::fs::symlink_metadata(&command).is_ok() {
        match crate::write::remove_file(&command) {
            Ok(()) => removed.command = true,
            Err(err) => failed = failed.or(Some(err)),
        }
    }
    failed.map_or(Ok(removed), Err)
}
