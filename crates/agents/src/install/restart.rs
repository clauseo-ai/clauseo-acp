//! What the next start needs from an install a quit cut short, kept in two small marker files,
//! and what an older build's Python left in `downloads/`:
//!
//! - `agents/<id>/.install-wanted`: written as an agent's job takes its locks, removed once that
//!   agent is `Ready`, when its job ends `Failed`, or when the lawyer pauses it. A quit leaves
//!   it, so the next start resumes the install with no press (`worker/thread.rs`).
//! - `agents/.python-blocked`: written when the OS refused to start the unpacked Python
//!   (`RunBlocked`), holding the blocker's word and the archive's name. The start's own retry of
//!   a missing Python skips it (another unpack and run would meet the same policy) and shows the
//!   refusal again; Try again removes it, and so does Python's `Ready`. An app update's new
//!   archive is not held back by an older one's refusal.
//!
//! The markers are hints, never trusted for more: a missing or unreadable one means "not
//! wanted" and "not blocked". Every write and removal is best effort, through no link.

use std::path::{Path, PathBuf};

use clauseo_protocol::AgentId;

use super::files::downloads_dir;
use crate::download::part_path;
use crate::log::InstallLog;
use crate::snapshot::PythonArchive;
use crate::state::{PythonBlocker, PythonFailure, PythonFailureKind, PythonState};
use crate::write;

/// The agent marker's file name, in `agents/<id>/`.
const WANTED: &str = ".install-wanted";
/// Python's marker, in `agents/`.
const BLOCKED: &str = ".python-blocked";
/// The prefix of every Python archive's file name.
const ARCHIVE_PREFIX: &str = "clauseo-python-";

fn wanted_path(data_dir: &Path, agent: &AgentId) -> PathBuf {
    data_dir.join("agents").join(agent.as_str()).join(WANTED)
}

fn blocked_path(data_dir: &Path) -> PathBuf {
    data_dir.join("agents").join(BLOCKED)
}

/// Writes `agent`'s marker: its install is under way, and a quit should not end it.
pub(crate) fn mark_wanted(data_dir: &Path, agent: &AgentId) {
    let path = wanted_path(data_dir, agent);
    if let Some(dir) = path.parent()
        && write::create_private_dirs_under(data_dir, dir).is_ok()
    {
        drop(write::write_atomic(&path, b"1\n"));
    }
}

/// Removes `agent`'s marker, when there is one.
pub(crate) fn clear_wanted(data_dir: &Path, agent: &AgentId) {
    let path = wanted_path(data_dir, agent);
    if path
        .parent()
        .is_some_and(|dir| write::real_dirs_under(data_dir, dir).unwrap_or(false))
    {
        drop(write::remove_file(&path));
    }
}

/// Whether `agent`'s marker is there: its install was cut short by a quit.
pub(crate) fn is_wanted(data_dir: &Path, agent: &AgentId) -> bool {
    let path = wanted_path(data_dir, agent);
    path.parent()
        .is_some_and(|dir| write::real_dirs_under(data_dir, dir).unwrap_or(false))
        && write::read_small(&path, 64).is_ok_and(|text| text.is_some())
}

/// Writes Python's marker for `archive`, with `by`'s word (`unknown` when none was read).
pub(crate) fn mark_blocked(data_dir: &Path, archive: &PythonArchive, by: Option<PythonBlocker>) {
    let agents = data_dir.join("agents");
    if write::real_dirs_under(data_dir, &agents).unwrap_or(false) {
        let word = by.unwrap_or(PythonBlocker::Unknown).word();
        let text = format!("{word} {}\n", archive.name());
        drop(write::write_atomic(
            &blocked_path(data_dir),
            text.as_bytes(),
        ));
    }
}

/// Removes Python's marker, when there is one.
pub(crate) fn clear_blocked(data_dir: &Path) {
    if write::real_dirs_under(data_dir, &data_dir.join("agents")).unwrap_or(false) {
        drop(write::remove_file(&blocked_path(data_dir)));
    }
}

/// What refused `archive` at an earlier start, when its marker is there and names it.
pub(crate) fn blocked(data_dir: &Path, archive: Option<&PythonArchive>) -> Option<PythonBlocker> {
    let archive = archive?;
    if !write::real_dirs_under(data_dir, &data_dir.join("agents")).unwrap_or(false) {
        return None;
    }
    let text = write::read_small(&blocked_path(data_dir), 512).ok()??;
    let (word, name) = text.trim().split_once(' ')?;
    if name != archive.name() {
        return None;
    }
    Some(match word {
        "app_control" => PythonBlocker::AppControl,
        "applocker" => PythonBlocker::AppLocker,
        _ => PythonBlocker::Unknown,
    })
}

/// Python's state at a start that finds its marker beside the `Ready` `agent`: `Failed` as
/// `RunBlocked` with what refused it, so the window says so again instead of a download.
pub(crate) fn blocked_state(by: PythonBlocker, agent: &AgentId) -> PythonState {
    let mut log = InstallLog::new(agent);
    log.push("Python was refused at an earlier start; Try again runs it again".to_owned());
    log.push(format!("blocked by: {}", by.word()));
    log.push(format!(
        "result: {} after 0 tries",
        PythonFailureKind::RunBlocked.word()
    ));
    PythonState::Failed(PythonFailure {
        kind: PythonFailureKind::RunBlocked,
        blocked_by: Some(by),
        log,
    })
}

/// Removes every Python archive in `downloads/`, and its `.part`, but `keep`'s: an app update's
/// new pin leaves the old one's behind. Run at the start with `.lock-python` held. Only a real
/// `downloads/` folder is listed, through no link; files alone, never a folder.
pub(crate) fn remove_stale_archives(data_dir: &Path, keep: Option<&PythonArchive>) {
    let downloads = downloads_dir(data_dir);
    if !write::real_dirs_under(data_dir, &downloads).unwrap_or(false) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(&downloads) else {
        return;
    };
    let kept = keep.map(|keep| {
        let dest = downloads.join(keep.name());
        (dest.clone(), part_path(&dest))
    });
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let archive = name.starts_with(ARCHIVE_PREFIX)
            && (name.ends_with(".tar.gz") || name.ends_with(".tar.gz.part"));
        let path = entry.path();
        let current = kept
            .as_ref()
            .is_some_and(|(dest, part)| path == *dest || path == *part);
        if archive && !current && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            drop(write::remove_file(&path));
        }
    }
}

#[cfg(test)]
mod tests;
