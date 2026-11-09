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
//! notes are merged at the start ([`moves::merge_notes`]): each entry whose name the new folder
//! lacks moves in one rename, a clash or a link stays, and the old folder goes once it is empty.
//! Nothing is followed through a link: an old folder that is a link, or is reached through one,
//! is never moved, and the launch that needs it is refused rather than given an empty new folder
//! ([`Unmovable`]); old notes that hold a link move entry by entry, the links left behind
//! ([`moves`]). Every rename is a no-replace rename. The log gets counts and kinds, never a path.
//!
//! The store's schema 6 (`crates/store/src/migrations/0006_agent_layout.sql`) marks the move:
//! the store opens, and so moves to 6, before [`migrate`] runs, so a build before this layout
//! (whose Codex fence does not deny `<data>/notes`) refuses the data folder from then on, even
//! after a start cut short.
//!
//! TODO(#825): stage C's protected DACLs on the owned roots (`acl_plan/owned.rs`) know only the
//! old layout. Whichever of #825 and this lands second makes the first launch after a move walk
//! the moved folders strictly (so they lose the ACL entries they inherited in `agent-state`),
//! creates the new roots through #825's protect, and adds `claude-state` and `notes\<m>` to
//! `owned_roots`. Until then the move runs at the start, before any launch's protection step.

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

use clauseo_protocol::{AgentKind, ErrorCode};

use super::STATE_ROOT;
use crate::error::Error;
use crate::log_file::Cause;

mod moves;

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
    /// The old notes were merged into the new folder that existed already
    /// ([`moves::merge_notes`]): `moved` entries moved, `left` (clashes and links) stayed.
    Merged {
        /// Entries moved into the new folder.
        moved: usize,
        /// Entries left in the old folder.
        left: usize,
    },
    /// The old place cannot move and there is no new folder: the launch is refused.
    Unmovable(Unmovable),
}

/// Why an old folder cannot move: it is never followed or moved, and a launch that needs it is
/// refused ([`refusal`]) rather than started on an empty new folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unmovable {
    /// The old folder is itself a link (or a junction).
    Link,
    /// A folder above it, below `<data>`, is a link.
    ThroughLink,
    /// The old place holds a file, not a folder.
    NotAFolder,
}

impl Unmovable {
    /// The kind's name for the log.
    fn kind(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::ThroughLink => "through a link",
            Self::NotAFolder => "not a folder",
        }
    }

    /// What is at `old` (`meta` its own metadata, links not followed), when it cannot move.
    fn of(data: &Path, old: &Path, meta: &std::fs::Metadata) -> Option<Self> {
        if meta.file_type().is_symlink() {
            Some(Self::Link)
        } else if !meta.is_dir() {
            Some(Self::NotAFolder)
        } else if !real_below(data, old) {
            Some(Self::ThroughLink)
        } else {
            None
        }
    }
}

impl std::fmt::Display for Unmovable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the old folder cannot move: {}", self.kind())
    }
}

impl std::error::Error for Unmovable {}

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

/// Moves `new`'s old folder ([`old_place`]) to `new`, when the old one is a real folder
/// reached through no link below `data` and nothing is at `new`; `new`'s parent is made first,
/// through no link. One no-replace rename, but old notes holding a link move entry by entry
/// ([`moves::move_folder`]). Run at the start ([`migrate`]) and before a launch makes `new`
/// (Claude's state folder, the notes). An old place that cannot move, with no new folder, is an
/// error carrying [`Unmovable`], so the launch is refused ([`refusal`]). File system calls:
/// off the actor once it serves.
pub(crate) fn adopt(data: &Path, new: &Path) -> io::Result<Adopted> {
    match adopt_with(data, new, false)? {
        Adopted::Unmovable(why) => Err(io::Error::other(why)),
        adopted @ (Adopted::Nothing | Adopted::Moved | Adopted::Left | Adopted::Merged { .. }) => {
            Ok(adopted)
        }
    }
}

/// [`adopt`]; with `merge` (the start only, when no agent runs), old notes beside a new notes
/// folder that is a real folder are merged into it ([`moves::merge_notes`]).
fn adopt_with(data: &Path, new: &Path, merge: bool) -> io::Result<Adopted> {
    let Some(old) = old_place(data, new) else {
        return Ok(Adopted::Nothing);
    };
    let unmovable = match std::fs::symlink_metadata(&old) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return finish_staged(data, new),
        Err(error) => return Err(error),
        Ok(meta) => Unmovable::of(data, &old, &meta),
    };
    let notes = is_notes(data, new);
    match std::fs::symlink_metadata(new) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
        Ok(_) if unmovable.is_none() && merge && notes && real_below(data, new) => {
            let (moved, left) = moves::merge_notes(&old, new)?;
            return Ok(Adopted::Merged { moved, left });
        }
        Ok(_) => return Ok(Adopted::Left),
    }
    if let Some(why) = unmovable {
        return Ok(Adopted::Unmovable(why));
    }
    let parent = new
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent folder"))?;
    crate::write::create_private_dirs_under(data, parent)?;
    match moves::move_folder(&old, new, notes) {
        Ok(_left) => Ok(Adopted::Moved),
        // Another launch's prep moved it first.
        Err(error) if error.kind() == io::ErrorKind::NotFound && real_below(data, new) => {
            Ok(Adopted::Nothing)
        }
        Err(error) => Err(error),
    }
}

/// With no old folder: a staging folder an entry-by-entry move left whole (`moves::staging`)
/// becomes `new` when nothing is there yet; else nothing to do.
fn finish_staged(data: &Path, new: &Path) -> io::Result<Adopted> {
    let Some(staged) = moves::staging(new) else {
        return Ok(Adopted::Nothing);
    };
    if !real_below(data, &staged) || std::fs::symlink_metadata(new).is_ok() {
        return Ok(Adopted::Nothing);
    }
    moves::rename(&staged, new)?;
    moves::sync_parents(&staged, new);
    Ok(Adopted::Moved)
}

/// Whether `new` is a notes folder of the new layout, `<data>/notes/<matter>`.
fn is_notes(data: &Path, new: &Path) -> bool {
    new.parent() == Some(data.join(NOTES_ROOT).as_path())
}

/// The launch's refusal when `what` (`"Claude Code's folder"`, `"the matter's notes"`) did not
/// move to the new layout, `agent.launch_failed`: an old folder that is a link or a file
/// ([`Unmovable`]) says so; one across volumes (`super::is_other_volume`) says the agent's files
/// are on another drive; on Windows, a file held open (`ERROR_SHARING_VIOLATION`,
/// `ERROR_LOCK_VIOLATION`, `ERROR_ACCESS_DENIED`) says to close Clauseo's other windows; any
/// other says it couldn't be prepared. The words name no path, the log only the kind or cause.
pub(crate) fn refusal(error: &io::Error, what: &str) -> Error {
    let refused = |text: String| Error::refused(ErrorCode::AgentLaunchFailed, text);
    if let Some(why) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<Unmovable>())
    {
        tracing::warn!(
            kind = why.kind(),
            "an agent's old folder cannot move; the launch is refused"
        );
        return refused(match why {
            Unmovable::NotAFolder => format!(
                "Clauseo couldn't move {what}: its old place in Clauseo's data folder is a file, \
                 not a folder. Move that file out of Clauseo's data folder, then try again."
            ),
            Unmovable::Link | Unmovable::ThroughLink => format!(
                "Clauseo couldn't move {what}: its old folder in Clauseo's data folder is a \
                 shortcut (a link), which Clauseo never follows. Put the folder itself in its \
                 place, then try again."
            ),
        });
    }
    if super::is_other_volume(error) {
        tracing::warn!("an agent's files are on another volume than the data folder; refused");
        return refused(format!(
            "Clauseo couldn't move {what}: the agent's files are on a different drive from \
             Clauseo's data folder. Put them on the same drive, then try again."
        ));
    }
    if is_busy(error) {
        tracing::warn!(error = %Cause(error), "an agent's files are in use; the launch is refused");
        return refused(format!(
            "Clauseo couldn't move {what}: a file in it is in use. Close Clauseo's other \
             windows or restart the computer, then try again."
        ));
    }
    tracing::warn!(error = %Cause(error), "an agent's files did not move; the launch is refused");
    refused(format!("Clauseo couldn't prepare {what}."))
}

/// Windows: whether `error` is a rename stopped by a file held open, `ERROR_SHARING_VIOLATION`
/// (32), `ERROR_LOCK_VIOLATION` (33) or `ERROR_ACCESS_DENIED` (5, what a folder holding an
/// open file gives). Never on Unix, where an open file does not stop a rename.
fn is_busy(error: &io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33))
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
    /// Old places that cannot move ([`Unmovable`]) with no new folder; their launches are
    /// refused.
    pub(crate) unmovable: usize,
    /// Moves that failed, tried again at the next start and at the folder's next launch.
    pub(crate) failed: usize,
}

/// Moves every matter's Claude state folder and notes out of `<data>/agent-state` to the new
/// layout ([`adopt`] for each matter folder there that is a real folder with a UTF-8 name). Run
/// once at the host's start, with the store's lock held and before the actor serves: no agent
/// runs, so old notes beside new ones are merged here ([`moves::merge_notes`]). A missing
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
                Ok(Adopted::Unmovable(why)) => {
                    counts.unmovable += 1;
                    tracing::warn!(kind = why.kind(), "a matter's old agent state cannot move");
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
            unmovable = counts.unmovable,
            failed = counts.failed,
            "matters' Claude state and notes moved out of the agent state folder"
        );
    }
    counts
}

#[cfg(test)]
pub(crate) mod tests;
