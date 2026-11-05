//! The set-aside's logic, over a [`Volume`]: what a launch records, what the after-session pass
//! and the launch's check of an open record move, the walk's single move, the list and the
//! restore. No Windows call, so it builds and is tested on every OS.

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

use clauseo_protocol::MatterId;

use super::files::{FileId, Place, Record, io_failure};
use super::{Entry, Reason, SetAsideFailure};
use crate::PROTECTED_NAMES;

/// Also checked: hooks in a `.git` the agent made would run in the lawyer's own git.
const ALSO: [&str; 1] = [".git"];

/// How many names an item tries in its pass's folder: its own, then `~2` up to `~9`.
const NAMES_TRIED: u32 = 9;

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

/// Each protected name at a matter's root.
fn names() -> impl Iterator<Item = &'static str> {
    PROTECTED_NAMES.iter().chain(&ALSO).copied()
}

/// See [`super::before_launch`]; `root` is the matter's folder.
pub(super) fn before_launch(
    volume: &impl Volume,
    root: &Path,
    data_dir: &Path,
    matter: &MatterId,
    now: SystemTime,
) -> Result<Vec<Entry>, SetAsideFailure> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    let mut moved = Vec::new();
    if let Some(record) = place.read_record()?
        && record.open
    {
        tracing::info!("the last session didn't end cleanly; checking its protected names");
        check(volume, &place, root, &record, now, &mut moved)?;
    }
    let names = snapshot(volume, root)?;
    place.write_record(&Record::new(*matter, true, names))?;
    Ok(moved)
}

/// See [`super::after_session`]. No record at all is [`SetAsideFailure::Record`]: there is
/// nothing to tell a change by.
pub(super) fn after_session(
    volume: &impl Volume,
    root: &Path,
    data_dir: &Path,
    matter: &MatterId,
    now: SystemTime,
) -> Result<Vec<Entry>, SetAsideFailure> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    let record = place.read_record()?.ok_or(SetAsideFailure::Record)?;
    if !record.open {
        return Ok(Vec::new());
    }
    let mut moved = Vec::new();
    check(volume, &place, root, &record, now, &mut moved)?;
    let names = snapshot(volume, root)?;
    place.write_record(&Record::new(*matter, false, names))?;
    Ok(moved)
}

/// See [`super::set_aside_path`].
pub(super) fn set_aside_path(
    volume: &impl Volume,
    data_dir: &Path,
    matter: &MatterId,
    path: &Path,
    reason: Reason,
    now: SystemTime,
) -> Result<Entry, SetAsideFailure> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    set_aside(volume, &place, path, reason, now)
}

/// See [`super::list_set_aside`].
pub(super) fn list(data_dir: &Path, matter: &MatterId) -> Result<Vec<Entry>, SetAsideFailure> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    Ok(place.read_manifest()?.entries)
}

/// See [`super::restore_set_aside`]. The manifest's own copy of the entry is used, found by its
/// number, and only when `entry` names the same item; its folder and name must each be one
/// plain path part.
pub(super) fn restore(
    volume: &impl Volume,
    data_dir: &Path,
    entry: &Entry,
) -> Result<(), SetAsideFailure> {
    let place = Place::new(data_dir, &entry.matter);
    let _held = place.lock()?;
    let mut manifest = place.read_manifest()?;
    let at = manifest
        .entries
        .iter()
        .position(|e| e.id == entry.id && e.from == entry.from && e.to == entry.to)
        .ok_or(SetAsideFailure::Gone)?;
    let found = manifest.entries.remove(at);
    if !one_part(&found.folder) || !one_part(&found.name) || !found.from.is_absolute() {
        return Err(SetAsideFailure::Record);
    }
    volume.move_back(
        &place.sandbox,
        &place.parts(&found.folder),
        &found.name,
        &found.from,
    )?;
    tracing::info!(reason = ?found.reason, "a set-aside item was restored");
    place.write_manifest(&manifest)
}

/// Sets aside each name `record` holds (and only protected names) that is there now with
/// another file ID than at launch, or that wasn't there then. A name whose ID can't be read for
/// want of permission is set aside as one the lawyer can't open. Every name is tried; the first
/// failure is returned after.
fn check(
    volume: &impl Volume,
    place: &Place,
    root: &Path,
    record: &Record,
    now: SystemTime,
    moved: &mut Vec<Entry>,
) -> Result<(), SetAsideFailure> {
    let mut first = None;
    for (name, was) in &record.names {
        if !names().any(|known| known == name) {
            continue;
        }
        let path = root.join(name);
        let reason = match volume.file_id(&path) {
            Ok(None) => continue,
            Ok(Some(id)) if Some(id) == *was => continue,
            Ok(Some(_)) if was.is_none() => Reason::New,
            Ok(Some(_)) => Reason::Changed,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Reason::LawyerCantOpen,
            Err(error) => {
                first.get_or_insert(io_failure(&error));
                continue;
            }
        };
        match set_aside(volume, place, &path, reason, now) {
            Ok(entry) => moved.push(entry),
            Err(failure) => {
                first.get_or_insert(failure);
            }
        }
    }
    if !moved.is_empty() {
        tracing::info!(
            count = moved.len(),
            "changed protected names were set aside"
        );
    }
    first.map_or(Ok(()), Err)
}

/// Each protected name at `root` and its file ID now, or `None` when absent.
fn snapshot(
    volume: &impl Volume,
    root: &Path,
) -> Result<BTreeMap<String, Option<FileId>>, SetAsideFailure> {
    names()
        .map(|name| {
            let id = volume
                .file_id(&root.join(name))
                .map_err(|e| io_failure(&e))?;
            Ok((name.to_owned(), id))
        })
        .collect()
}

/// Moves `from` into the pass's folder for `now`, under its own name or the first free `~n`
/// one, and adds its manifest entry. The lock is held.
fn set_aside(
    volume: &impl Volume,
    place: &Place,
    from: &Path,
    reason: Reason,
    now: SystemTime,
) -> Result<Entry, SetAsideFailure> {
    let text = from
        .to_str()
        .ok_or(SetAsideFailure::Io(io::ErrorKind::InvalidData))?;
    let own = from
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| one_part(name))
        .ok_or(SetAsideFailure::Io(io::ErrorKind::InvalidInput))?;
    let owner = volume.owner(from).ok();
    let folder = timestamp(now);
    let parts = place.parts(&folder);
    for n in 1..=NAMES_TRIED {
        let name = if n == 1 {
            own.to_owned()
        } else {
            format!("{own}~{n}")
        };
        match volume.move_into(from, &place.sandbox, &parts, &name) {
            Ok(()) => {}
            Err(SetAsideFailure::NameTaken) => continue,
            Err(other) => return Err(other),
        }
        let mut manifest = place.read_manifest()?;
        let entry = Entry {
            id: manifest.next,
            matter: place.matter(),
            from: text.into(),
            to: place.to(&folder, &name),
            folder,
            name,
            reason,
            owner,
            at: rfc3339(now),
        };
        manifest.next += 1;
        manifest.entries.push(entry.clone());
        place.write_manifest(&manifest)?;
        tracing::info!(?reason, "a protected name was set aside");
        return Ok(entry);
    }
    Err(SetAsideFailure::NameTaken)
}

/// Whether `text` is one plain path part: no separator, drive, `.` or `..`.
fn one_part(text: &str) -> bool {
    let mut parts = Path::new(text).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) && !text.contains(['/', '\\', ':'])
}

/// `now` in UTC as `YYYYMMDDTHHMMSS-mmmZ`: a pass's folder name.
pub(super) fn timestamp(now: SystemTime) -> String {
    let (year, month, day, secs, millis) = utc(now);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}-{millis:03}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60,
    )
}

/// `now` in UTC as RFC 3339 to the millisecond: `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub(super) fn rfc3339(now: SystemTime) -> String {
    let (year, month, day, secs, millis) = utc(now);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60,
    )
}

/// `now`'s UTC date, its seconds into the day and its milliseconds; the epoch for a time
/// before it.
fn utc(now: SystemTime) -> (u64, u64, u64, u64, u32) {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (year, month, day) = civil(secs / 86_400);
    (year, month, day, secs % 86_400, since.subsec_millis())
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}
