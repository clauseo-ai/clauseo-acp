//! Where each matter's agent state sits under `<data>` (R17 PR-D1), and the start-up move from
//! the layout before it. Codex's state folders stay in `<data>/agent-state/<matter>/codex-acp`
//! (Codex's databases store absolute paths into its home, so none of it moves); Claude's state
//! folders go to `<data>/claude-state/<matter>/<agent>` and the matters' notes to
//! `<data>/notes/<matter>`. So the jail can deny `agent-state` whole but the session's own Codex
//! home, and grant Claude's state and the notes exactly.
//!
//! The move ([`migrate`]) runs once at the host's start, after the store's lock is taken (so no
//! other host serves this data folder) and before the actor serves anything (so no launch, and
//! no agent of this host, runs yet). Each folder moves in one rename on the same volume
//! ([`adopt`]): the old name is there or the new one is, never half of a folder. A start cut short
//! leaves some matters moved and the rest where they were, and the next start moves the rest; a
//! folder already moved is skipped, so a second start does nothing. A move that fails (a file
//! held open on Windows) is tried again at the next start, and again at each launch that needs
//! the folder, on the launch-prep thread before the folder is made; the launch is refused while
//! it fails, so no agent ever starts on an empty new folder while its old one holds the matter's
//! transcripts or notes; a move across volumes is refused with its own words ([`refusal`]).
//! Where both an old and a new folder exist (a downgraded app made the old one again), the new
//! one is used. Claude's old state folder is left as it is, never merged or removed; the old
//! notes are merged at the start ([`merge_notes`]): each entry whose name the new folder lacks
//! moves in one rename, a clash or a link stays, and the old folder goes once it is empty.
//! Nothing is followed through a link: a link in the old or new place is never moved, and every
//! folder on the way below `<data>` must be a real one. The log gets counts, never a path.

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

use clauseo_protocol::{AgentKind, ErrorCode};

use super::STATE_ROOT;
use crate::error::Error;
use crate::log_file::Cause;

/// The folder under `<data>` holding every matter's Claude state folders:
/// `<data>/claude-state/<matter>/<agent>`.
pub(crate) const CLAUDE_STATE_ROOT: &str = "claude-state";

/// The folder under `<data>` holding every matter's notes (R22): `<data>/notes/<matter>`.
pub(crate) const NOTES_ROOT: &str = "notes";

/// Every folder under `<data>` holding a matter's agent state or notes.
pub(crate) const STATE_ROOTS: [&str; 3] = [STATE_ROOT, CLAUDE_STATE_ROOT, NOTES_ROOT];

/// The notes folder's name in `<data>/agent-state/<matter>` before R17 PR-D1.
const OLD_NOTES: &str = "notes";

/// `<data>/<root>/<matter>/<agent>`: Claude's under [`CLAUDE_STATE_ROOT`], any other agent's
/// (Codex's) under [`STATE_ROOT`].
pub(super) fn state_at(data: &Path, matter: &str, agent: &str) -> PathBuf {
    let root = if agent == AgentKind::Claude.id() {
        CLAUDE_STATE_ROOT
    } else {
        STATE_ROOT
    };
    data.join(root).join(matter).join(agent)
}

/// `<data>/notes/<matter>`.
pub(crate) fn notes_at(data: &Path, matter: &str) -> PathBuf {
    data.join(NOTES_ROOT).join(matter)
}

/// Where `new` (a folder of the new layout under `data`) was before R17 PR-D1:
/// `claude-state/<m>/<agent>` was `agent-state/<m>/<agent>`, `notes/<m>` was
/// `agent-state/<m>/notes`. `None` for any other path, which never moved.
fn old_place(data: &Path, new: &Path) -> Option<PathBuf> {
    let names: Vec<&OsStr> = new
        .strip_prefix(data)
        .ok()?
        .components()
        .map(|part| match part {
            Component::Normal(name) => Some(name),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => None,
        })
        .collect::<Option<_>>()?;
    let old = data.join(STATE_ROOT);
    match names.as_slice() {
        [root, matter, agent] if *root == CLAUDE_STATE_ROOT => Some(old.join(matter).join(agent)),
        [root, matter] if *root == NOTES_ROOT => Some(old.join(matter).join(OLD_NOTES)),
        _ => None,
    }
}

/// What [`adopt`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Adopted {
    /// No old folder: nothing to move (moved already, or never made).
    Nothing,
    /// The old folder is now the new one.
    Moved,
    /// The old place holds something that stays: a link or a file (never moved), or a folder
    /// beside a new one that exists already (the new one is used).
    Left,
    /// The old notes were merged into the new folder that existed already ([`merge_notes`]):
    /// `moved` entries moved, `left` (clashes and links) stayed.
    Merged {
        /// Entries moved into the new folder.
        moved: usize,
        /// Entries left in the old folder.
        left: usize,
    },
}

/// Whether every name of `path` below `data`, `path` included, is a real folder (never a link
/// or a junction). `false` when one is missing.
fn real_below(data: &Path, path: &Path) -> bool {
    let Ok(below) = path.strip_prefix(data) else {
        return false;
    };
    let mut at = data.to_owned();
    below.components().all(|part| {
        at.push(part);
        crate::write::is_real_dir(&at)
    })
}

/// Moves `new`'s old folder ([`old_place`]) to `new` in one rename, when the old one is a real
/// folder reached through no link below `data` and nothing is at `new`; `new`'s parent is made
/// first, through no link. Run at the start ([`migrate`]) and before a launch makes `new`
/// (Claude's state folder, the notes). File system calls: off the actor once it serves.
pub(crate) fn adopt(data: &Path, new: &Path) -> io::Result<Adopted> {
    adopt_with(data, new, false)
}

/// [`adopt`]; with `merge` (the start only, when no agent runs), old notes beside a new notes
/// folder that is a real folder are merged into it ([`merge_notes`]).
fn adopt_with(data: &Path, new: &Path, merge: bool) -> io::Result<Adopted> {
    let Some(old) = old_place(data, new) else {
        return Ok(Adopted::Nothing);
    };
    match std::fs::symlink_metadata(&old) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Adopted::Nothing),
        Err(error) => return Err(error),
        Ok(_) if !real_below(data, &old) => return Ok(Adopted::Left),
        Ok(_) => {}
    }
    match std::fs::symlink_metadata(new) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
        Ok(_) if merge && is_notes(data, new) && real_below(data, new) => {
            return merge_notes(&old, new);
        }
        Ok(_) => return Ok(Adopted::Left),
    }
    let parent = new
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent folder"))?;
    crate::write::create_private_dirs_under(data, parent)?;
    match rename(&old, new) {
        Ok(()) => Ok(Adopted::Moved),
        // Another launch's prep moved it first.
        Err(error) if error.kind() == io::ErrorKind::NotFound && real_below(data, new) => {
            Ok(Adopted::Nothing)
        }
        Err(error) => Err(error),
    }
}

/// Whether `new` is a notes folder of the new layout, `<data>/notes/<matter>`.
fn is_notes(data: &Path, new: &Path) -> bool {
    new.parent() == Some(data.join(NOTES_ROOT).as_path())
}

/// Merges the old notes folder `old` into the new one `new`, which a downgraded app's notes
/// left beside it: each entry of `old` whose name `new` lacks moves in one rename (a folder
/// whole), so none is ever split or overwritten; a name `new` has already (a clash) or an entry
/// that is a link stays where it is. `old` is removed once empty. Run at the start only, when
/// no agent runs, so nothing makes a name in `new` between the check and the rename. A failure
/// stops the merge; the next start goes on from where it stopped.
fn merge_notes(old: &Path, new: &Path) -> io::Result<Adopted> {
    let (mut moved, mut left) = (0, 0);
    for entry in std::fs::read_dir(old)? {
        let entry = entry?;
        let target = new.join(entry.file_name());
        let free = matches!(
            std::fs::symlink_metadata(&target),
            Err(error) if error.kind() == io::ErrorKind::NotFound
        );
        if !free || entry.file_type()?.is_symlink() {
            left += 1;
            continue;
        }
        rename(&entry.path(), &target)?;
        moved += 1;
    }
    if left == 0 {
        crate::write::remove_empty_dir(old)?;
    }
    Ok(Adopted::Merged { moved, left })
}

/// The one rename of a move or a merge (`write::rename_folder`). In unit tests a failure of a
/// chosen kind can stand in for it ([`tests::fail_renames_with`]): a move across volumes.
fn rename(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(test)]
    if let Some(kind) = tests::FAIL_RENAMES.with(std::cell::Cell::get) {
        return Err(io::Error::from(kind));
    }
    crate::write::rename_folder(from, to)
}

/// The launch's refusal when `what` (`"Claude Code's folder"`, `"the matter's notes"`) did not
/// move to the new layout: one across volumes (`super::is_other_volume`) says the agent's files
/// are on another drive; any other says it couldn't be prepared. `agent.launch_failed`; the
/// words name no path, the log only the cause.
pub(crate) fn refusal(error: &io::Error, what: &str) -> Error {
    if super::is_other_volume(error) {
        tracing::warn!("an agent's files are on another volume than the data folder; refused");
        return Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!(
                "Clauseo couldn't move {what}: the agent's files are on a different drive from \
                 Clauseo's data folder. Put them on the same drive, then try again."
            ),
        );
    }
    tracing::warn!(error = %Cause(error), "an agent's files did not move; the launch is refused");
    Error::refused(
        ErrorCode::AgentLaunchFailed,
        format!("Clauseo couldn't prepare {what}."),
    )
}

/// What [`migrate`] did, counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Migrated {
    /// Folders moved to the new layout.
    pub(crate) moved: usize,
    /// Old places left as they are ([`Adopted::Left`]).
    pub(crate) left: usize,
    /// Old notes entries merged into a new notes folder ([`Adopted::Merged`]).
    pub(crate) merged: usize,
    /// Old notes entries left beside a new notes folder: a clash or a link.
    pub(crate) clashes: usize,
    /// Moves that failed, tried again at the next start and at the folder's next launch.
    pub(crate) failed: usize,
}

/// Moves every matter's Claude state folder and notes out of `<data>/agent-state` to the new
/// layout ([`adopt`] for each matter folder there that is a real folder with a UTF-8 name). Run
/// once at the host's start, with the store's lock held and before the actor serves: no agent
/// runs, so old notes beside new ones are merged here ([`merge_notes`]). A missing
/// `agent-state`, or one that is a link, moves nothing. Logs counts only.
pub(crate) fn migrate(data: &Path) -> Migrated {
    let mut counts = Migrated::default();
    let root = data.join(STATE_ROOT);
    if !crate::write::is_real_dir(&root) {
        return counts;
    }
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(error = %Cause(&error), "the agent state folder was not listed");
            return counts;
        }
    };
    for entry in entries.flatten() {
        let folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
        let name = entry.file_name();
        let (true, Some(matter)) = (folder, name.to_str()) else {
            continue;
        };
        for new in [
            state_at(data, matter, AgentKind::Claude.id()),
            notes_at(data, matter),
        ] {
            match adopt_with(data, &new, true) {
                Ok(Adopted::Nothing) => {}
                Ok(Adopted::Moved) => counts.moved += 1,
                Ok(Adopted::Left) => counts.left += 1,
                Ok(Adopted::Merged { moved, left }) => {
                    counts.merged += moved;
                    counts.clashes += left;
                }
                Err(error) => {
                    counts.failed += 1;
                    tracing::warn!(error = %Cause(&error), "a matter's agent state did not move");
                }
            }
        }
    }
    if counts != Migrated::default() {
        tracing::info!(
            moved = counts.moved,
            left = counts.left,
            merged = counts.merged,
            clashes = counts.clashes,
            failed = counts.failed,
            "matters' Claude state and notes moved out of the agent state folder"
        );
    }
    counts
}

#[cfg(test)]
pub(crate) mod tests;
