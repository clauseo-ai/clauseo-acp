//! The unclean-session marker: `<data_dir>\sandbox\run\<id>.session`, where `<id>` is the launch
//! file's 16 hex. The outer writes it as soon as it has read the launch, before anything runs as
//! the pool user, and removes it only at a clean end: the inner exited (no failure, and no cancel
//! whose agent outlived its grace), reported the agent's exit, the end sweeper exited 0, and no
//! sweep found an escapee (`end::EndSweep::clean`). A crash, a cancel whose agent outlived its
//! grace, a failure or an escapee leaves it, and
//! [`unclean_sessions`] lists what was left, so Clauseo can make sure no process of that pool
//! user lives on (0049; the `\Clauseo\SandboxStop` task, B's PR 3, is its caller).
//!
//! It lives in the run folder, which only the desktop user, `SYSTEM` and `Administrators` may
//! open (`write::make_sandbox_dir`): the pool user can neither write nor remove it, and gets no
//! say in whether a session counts as clean. It is written as the launch file is (atomically,
//! then given the folder's three entries as its own protected DACL and the desktop user as its
//! owner) and holds the pool user's SID, nothing else.
//!
//! The names, the paths and the listing hold no Windows call, so their tests run on every OS.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::RUN_DIR;
use crate::windows::matters::SANDBOX_DIR;
use crate::windows::sids::Sid;
use crate::windows::write;

/// The marker's suffix, after the launch's id.
pub const MARKER_SUFFIX: &str = ".session";
/// The largest marker read.
const MARKER_CAP: u64 = 4096;

/// What a marker holds.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    /// The session's pool user.
    pool_user: Sid,
}

/// A session whose outer left its marker: it crashed, was cancelled or failed, or a sweep found
/// an escapee.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncleanSession {
    /// The launch's id, 16 lowercase hex.
    pub id: String,
    /// Its pool user; `None` when the marker can't be read, or isn't the desktop user's alone.
    pub pool_user: Option<Sid>,
}

/// The marker's path for the launch `id`: `<data_dir>\sandbox\run\<id>.session`.
pub fn marker_path(data_dir: &Path, id: &str) -> PathBuf {
    data_dir
        .join(SANDBOX_DIR)
        .join(RUN_DIR)
        .join(format!("{id}{MARKER_SUFFIX}"))
}

/// The launch id a marker's file name gives: `<16 lowercase hex>.session`; `None` for any other
/// name.
pub fn marker_id(name: &str) -> Option<&str> {
    name.strip_suffix(MARKER_SUFFIX).filter(|hex| is_id(hex))
}

/// The launch id of the launch file `path` (`<16 lowercase hex>.json`), the marker's.
pub fn launch_id(path: &Path) -> Option<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".json"))
        .filter(|hex| is_id(hex))
}

/// Writes the marker for the launch `id` and its pool user, in the run folder (which must
/// exist: the launch file came from it); its path.
pub fn write_marker(data_dir: &Path, id: &str, pool_user: &Sid) -> io::Result<PathBuf> {
    if !is_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a marker's id is 16 lowercase hex",
        ));
    }
    let marker = Marker {
        pool_user: pool_user.clone(),
    };
    let bytes = serde_json::to_vec(&marker).map_err(io::Error::other)?;
    let path = marker_path(data_dir, id);
    write::write_atomic(&path, &bytes)?;
    // As the launch file: the folder's three entries as its own protected DACL, and the desktop
    // user its owner.
    write::check_owned_file(&path)?;
    #[cfg(windows)]
    crate::windows::private_dir::own_file(&path)?;
    Ok(path)
}

/// Removes the marker at `path`, as the clean end does; one already gone is fine.
pub fn remove_marker(path: &Path) -> io::Result<()> {
    write::remove_owned_file(path)
}

/// The sessions whose marker is left in `<data_dir>\sandbox\run\`, sorted by id; none when the
/// folder is missing. Each marker is read without being changed; one that can't be read still
/// lists its session, with no pool user.
pub fn unclean_sessions(data_dir: &Path) -> io::Result<Vec<UncleanSession>> {
    let run = data_dir.join(SANDBOX_DIR).join(RUN_DIR);
    let entries = match std::fs::read_dir(&run) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut sessions = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(marker_id) else {
            continue;
        };
        let pool_user = write::read_owned_file_unchanged(&entry.path(), MARKER_CAP)
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok())
            .map(|marker| marker.pool_user);
        sessions.push(UncleanSession {
            id: id.to_owned(),
            pool_user,
        });
    }
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(sessions)
}

/// Whether `hex` is a launch id: 16 lowercase hex.
fn is_id(hex: &str) -> bool {
    hex.len() == 16
        && hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests;
