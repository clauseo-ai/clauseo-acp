//! The unclean-session marker: `<data_dir>\sandbox\run\<id>.session`, where `<id>` is the launch
//! file's 16 hex. The outer writes it open ([`MarkerState::Open`]) as soon as it has read the
//! launch, before anything runs as the pool user ([`marked`]: a marker that can't be written
//! refuses the launch). At its end it never waits for a task's run; it writes what the marker
//! waits for instead (`end::SessionEnd::finish`):
//!
//! - [`MarkerState::AwaitingSweep`] `{ sweep: M }`: every end without an escapee, `M` the
//!   `SweepNext` read once the job ended. Clean once `SweepSequence >= M`: a sweep reserved its
//!   number after the session could launch nothing more, and completed.
//! - [`MarkerState::AwaitingStop`] `{ stop, sweep }`: a session a sweep found an escapee in, `stop`
//!   the `StopNext` read then. Clean once `StopSequence >= stop` and `SweepSequence >= sweep`.
//! - [`MarkerState::Open`] left standing: the outer died (the host's job end after its
//!   `stdin_grace`, a crash) or couldn't read a number. The pool user's next logon or the
//!   desktop's start makes it `AwaitingStop`, reading both numbers then (`unclean.rs`).
//!
//! An absent `…Sequence` counts as 0. A clean marker is removed by whoever finds it clean. The
//! numbers are read without the set-up lock, which only an administrator can take; why that is
//! sound is in `setup::pool_steps::sequence`. [`unclean_sessions`] lists the markers.
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
    /// The session's pool user; `None` only in a marker the desktop's start wrote in place of
    /// one it couldn't read, which counts for every pool user.
    pool_user: Option<Sid>,
    /// What it waits for; absent in a marker written before the states, which reads as open.
    #[serde(default)]
    state: MarkerState,
}

/// What a marker waits for before it is clean.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MarkerState {
    /// Written at the session's start; standing once its outer is gone, it is never clean.
    #[default]
    Open,
    /// The session ended with no escapee: a sweep that reserved `sweep` or later must complete.
    AwaitingSweep {
        /// `SweepNext` as read once the session could launch nothing more.
        sweep: u64,
    },
    /// An escapee, or an outer that died: a stop and a sweep must complete.
    AwaitingStop {
        /// `StopNext` as read once the session could launch nothing more.
        stop: u64,
        /// `SweepNext` as read then.
        sweep: u64,
    },
}

/// The pool tasks' completed sequences, as read now: `StopSequence` and `SweepSequence`, each 0
/// when absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sequences {
    /// `StopSequence`.
    pub stop: u64,
    /// `SweepSequence`.
    pub sweep: u64,
}

impl MarkerState {
    /// The stop this marker waits for, if any.
    pub fn stop(self) -> Option<u64> {
        match self {
            MarkerState::AwaitingStop { stop, .. } => Some(stop),
            MarkerState::Open | MarkerState::AwaitingSweep { .. } => None,
        }
    }

    /// The sweep this marker waits for, if any.
    pub fn sweep(self) -> Option<u64> {
        match self {
            MarkerState::AwaitingSweep { sweep } | MarkerState::AwaitingStop { sweep, .. } => {
                Some(sweep)
            }
            MarkerState::Open => None,
        }
    }

    /// Whether `done` meets every number this marker waits for: never when open.
    pub fn clean(self, done: Sequences) -> bool {
        match self {
            MarkerState::Open => false,
            MarkerState::AwaitingSweep { sweep } => done.sweep >= sweep,
            MarkerState::AwaitingStop { stop, sweep } => done.stop >= stop && done.sweep >= sweep,
        }
    }
}

/// A session whose marker is left: one still running, or one waiting for a task's run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UncleanSession {
    /// The launch's id, 16 lowercase hex.
    pub id: String,
    /// Its pool user; `None` when the marker can't be read, isn't the desktop user's alone, or
    /// names none (it counts for every pool user then).
    pub pool_user: Option<Sid>,
    /// What it waits for; `None` when it can't be read.
    pub state: Option<MarkerState>,
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

/// Writes the open marker for the launch `id` and its pool user, in the run folder (which must
/// exist: the launch file came from it); its path.
pub fn write_marker(data_dir: &Path, id: &str, pool_user: &Sid) -> io::Result<PathBuf> {
    let path = marker_path(data_dir, id);
    write_marker_at(&path, Some(pool_user), MarkerState::Open)?;
    Ok(path)
}

/// Writes the marker at `path` (`<16 hex>.session` in the run folder) as `state`, for
/// `pool_user`, in place of what it held.
pub fn write_marker_at(path: &Path, pool_user: Option<&Sid>, state: MarkerState) -> io::Result<()> {
    let named = path.file_name().and_then(|name| name.to_str());
    if named.and_then(marker_id).is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a marker's id is 16 lowercase hex",
        ));
    }
    let marker = Marker {
        pool_user: pool_user.cloned(),
        state,
    };
    let bytes = serde_json::to_vec(&marker).map_err(io::Error::other)?;
    write::write_atomic(path, &bytes)?;
    // As the launch file: the folder's three entries as its own protected DACL, and the desktop
    // user its owner.
    write::check_owned_file(path)?;
    #[cfg(windows)]
    crate::windows::private_dir::own_file(path)?;
    Ok(())
}

/// The outer's order once it read the launch `launch_file`: writes the session's marker for
/// `pool_user` in `data_dir`, then runs `session` (everything that runs as the pool user) with
/// its path. A marker that can't be written, or a launch file whose name gives no id, refuses the
/// launch: `session` never runs, and the error comes back. No session runs unmarked, so none can
/// leave a process of the pool user that no marker names.
pub(crate) fn marked<T>(
    data_dir: &Path,
    launch_file: &Path,
    pool_user: &Sid,
    session: impl FnOnce(PathBuf) -> T,
) -> io::Result<T> {
    let id = launch_id(launch_file).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the launch file's name gives no id",
        )
    })?;
    let marker = write_marker(data_dir, id, pool_user)?;
    Ok(session(marker))
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
        let marker = write::read_owned_file_unchanged(&entry.path(), MARKER_CAP)
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok());
        sessions.push(UncleanSession {
            id: id.to_owned(),
            pool_user: marker.as_ref().and_then(|marker| marker.pool_user.clone()),
            state: marker.map(|marker| marker.state),
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
