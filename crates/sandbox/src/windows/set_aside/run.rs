//! The set-aside's passes over a [`Volume`]: what a launch records and checks, what the
//! after-session pass checks, the walk's single move, the list and the restore. No Windows
//! call, so it builds and is tested on every OS.

use std::collections::BTreeSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use clauseo_protocol::MatterId;

use super::check::{check, snapshot};
use super::files::{FileId, Place, Record};
use super::moves::{load, set_aside};
use super::{Entry, Reason, SetAsideFailure};
use crate::windows::Found;

/// The file-system calls the set-aside makes: NTFS on Windows (`ntfs.rs`), and the tests'
/// stand-in elsewhere.
pub(super) trait Volume {
    /// The file ID of `path` itself, never what a link at it names; `None` when nothing is
    /// there.
    fn file_id(&self, path: &Path) -> io::Result<Option<FileId>>;

    /// The SID of the owner of `path` itself, as text.
    fn owner(&self, path: &Path) -> io::Result<String>;

    /// Moves the item at `from` (itself, not what a link names; its final path must be `from`)
    /// into the folder `parts` below `sandbox`, each part made or opened in turn without
    /// following a link, as `name`. Never replaces: a taken name is
    /// [`SetAsideFailure::NameTaken`], a missing item [`SetAsideFailure::Gone`].
    fn move_into(
        &self,
        from: &Path,
        sandbox: &Path,
        parts: &[String],
        name: &str,
    ) -> Result<(), SetAsideFailure>;

    /// Moves `name` in the folder `parts` below `sandbox` (opened as for
    /// [`Volume::move_into`], never made) back to `to`, whose folder's final path must be
    /// `to`'s parent. Never replaces.
    fn move_back(
        &self,
        sandbox: &Path,
        parts: &[String],
        name: &str,
        to: &Path,
    ) -> Result<(), SetAsideFailure>;
}

/// What a pass looks at: the matter's folder, and the protected names a walk found in it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Seen<'a> {
    /// The matter's folder, as the policy holds it.
    pub(super) root: &'a Path,
    /// The `.git`, `.claude` and `.mcp.json` the walk met, at any depth.
    pub(super) found: &'a [Found],
}

/// Why a pass failed, and the item it failed on, when it was one (N11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Refused {
    pub(super) failure: SetAsideFailure,
    /// The item that couldn't be read or moved.
    pub(super) at: Option<PathBuf>,
}

impl Refused {
    /// `failure` on the item `at`.
    pub(super) fn at(failure: SetAsideFailure, at: &Path) -> Refused {
        Refused {
            failure,
            at: Some(at.to_owned()),
        }
    }

    /// As the crate's error.
    #[cfg(windows)]
    pub(super) fn into_error(self) -> crate::Error {
        crate::Error::SetAside {
            failure: self.failure,
            at: self.at,
        }
    }
}

impl From<SetAsideFailure> for Refused {
    fn from(failure: SetAsideFailure) -> Refused {
        Refused { failure, at: None }
    }
}

/// See [`super::before_launch`].
pub(super) fn before_launch(
    volume: &impl Volume,
    seen: &Seen<'_>,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    now: SystemTime,
) -> Result<Vec<Entry>, Refused> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    let mut moved = Vec::new();
    let mut sessions = match place.read_record()? {
        Some(record) if !record.sessions.is_empty() => {
            tracing::info!(
                open = record.sessions.len(),
                "a session of this matter is open; checking its protected names"
            );
            check(volume, &place, seen, &record, now, &mut moved)?;
            record.sessions
        }
        Some(record) => record.sessions,
        None => BTreeSet::new(),
    };
    sessions.insert(session.to_owned());
    let names = snapshot(volume, seen, &moved)?;
    place.write_record(&Record::new(*matter, sessions, names))?;
    Ok(moved)
}

/// See [`super::after_session`]. No record at all is [`SetAsideFailure::Record`]: there is
/// nothing to tell a change by.
pub(super) fn after_session(
    volume: &impl Volume,
    seen: &Seen<'_>,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    now: SystemTime,
) -> Result<Vec<Entry>, Refused> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    let record = place.read_record()?.ok_or(SetAsideFailure::Record)?;
    let mut moved = Vec::new();
    check(volume, &place, seen, &record, now, &mut moved)?;
    let mut sessions = record.sessions;
    sessions.remove(session);
    let names = snapshot(volume, seen, &moved)?;
    place.write_record(&Record::new(*matter, sessions, names))?;
    Ok(moved)
}

/// See [`super::set_aside_path`]: the item under its own name in the pass's folder.
pub(super) fn set_aside_path(
    volume: &impl Volume,
    data_dir: &Path,
    matter: &MatterId,
    path: &Path,
    reason: Reason,
    now: SystemTime,
) -> Result<Entry, Refused> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock().map_err(|failure| Refused::at(failure, path))?;
    set_aside(volume, &place, path, &[], reason, now)
}

/// See [`super::list_set_aside`].
pub(super) fn list(
    volume: &impl Volume,
    data_dir: &Path,
    matter: &MatterId,
) -> Result<Vec<Entry>, Refused> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    Ok(load(volume, &place)?.entries)
}

/// See [`super::restore_set_aside`]. The manifest's own copy of the entry is used, found by its
/// number, and only when `entry` names the same item; its folder, the folders within it and
/// its name must each be one plain path part.
pub(super) fn restore(volume: &impl Volume, data_dir: &Path, entry: &Entry) -> Result<(), Refused> {
    let place = Place::new(data_dir, &entry.matter);
    let _held = place.lock()?;
    let mut manifest = load(volume, &place)?;
    let at = manifest
        .entries
        .iter()
        .position(|e| e.id == entry.id && e.from == entry.from && e.to == entry.to)
        .ok_or(SetAsideFailure::Gone)?;
    let found = manifest.entries.remove(at);
    let plain = one_part(&found.folder)
        && one_part(&found.name)
        && found.within.iter().all(|part| one_part(part));
    if !plain || !found.from.is_absolute() {
        return Err(SetAsideFailure::Record.into());
    }
    volume.move_back(
        &place.sandbox,
        &place.parts(&found.folder, &found.within),
        &found.name,
        &found.from,
    )?;
    tracing::info!(reason = ?found.reason, "a set-aside item was restored");
    Ok(place.write_manifest(&manifest)?)
}

/// Whether `text` is one plain path part: no separator, drive, `.` or `..`.
pub(super) fn one_part(text: &str) -> bool {
    let mut parts = Path::new(text).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) && !text.contains(['/', '\\', ':'])
}
