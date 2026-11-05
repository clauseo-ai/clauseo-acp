//! The check by change: each protected name now (those at the matter's root, read through the
//! [`Volume`], and the nested ones a walk found), compared with the record, and what is new or
//! changed set aside, each at its own path below the matter's root (N1).

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use super::files::{FileId, Place, Record, io_failure};
use super::moves::set_aside;
use super::run::{Refused, Seen, Volume};
use super::{Entry, Reason, SetAsideFailure};
use crate::PROTECTED_NAMES;

/// Also checked at the root: hooks in a `.git` the agent made would run in the lawyer's own git.
const ALSO: [&str; 1] = [".git"];

/// Each protected name checked at the matter's root.
fn root_names() -> impl Iterator<Item = &'static str> {
    PROTECTED_NAMES.iter().chain(&ALSO).copied()
}

/// What is at a protected name now.
#[derive(Debug, Clone, Copy)]
enum Now {
    /// Nothing.
    Absent,
    /// The file or folder with this ID.
    At(FileId),
    /// Something whose ID the lawyer can't read: its permissions shut the lawyer out.
    Shut,
    /// Its ID couldn't be read, or its path isn't text.
    Failed(SetAsideFailure),
}

/// One protected name: where it is, the folders it is in below the root, and what is there.
#[derive(Debug, Clone)]
struct Name {
    path: PathBuf,
    within: Vec<String>,
    now: Now,
    /// One of the root's names, read through the volume rather than found by the walk.
    at_root: bool,
}

/// Each protected name now, by its path below `seen.root` with its parts joined by `/`: each
/// one at the root, read through `volume`, then each one the walk found below the root. A found
/// one outside the root is ignored, and one at the root that the root's names cover is one
/// entry. Ordered so a folder comes before what is inside it.
fn names(volume: &impl Volume, seen: &Seen<'_>) -> BTreeMap<String, Name> {
    let mut names = BTreeMap::new();
    for name in root_names() {
        let path = seen.root.join(name);
        let now = match volume.file_id(&path) {
            Ok(None) => Now::Absent,
            Ok(Some(id)) => Now::At(id),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Now::Shut,
            Err(error) => Now::Failed(io_failure(&error)),
        };
        let within = Vec::new();
        let at_root = true;
        let name_now = Name {
            path,
            within,
            now,
            at_root,
        };
        names.insert(name.to_owned(), name_now);
    }
    for found in seen.found {
        let id = FileId {
            volume: found.volume,
            id: found.id,
        };
        let (key, within, now) = match below(seen.root, &found.path) {
            Below::Outside => {
                tracing::debug!("a protected name found outside the matter's root is ignored");
                continue;
            }
            Below::Parts(mut parts) => {
                let key = parts.join("/");
                parts.pop();
                (key, parts, Now::At(id))
            }
            Below::NotText => {
                let key = found.path.to_string_lossy().into_owned();
                let failed = Now::Failed(SetAsideFailure::Io(io::ErrorKind::InvalidData));
                (key, Vec::new(), failed)
            }
        };
        if root_names().any(|root| root.eq_ignore_ascii_case(&key)) {
            continue;
        }
        let path = found.path.clone();
        let at_root = false;
        let name = Name {
            path,
            within,
            now,
            at_root,
        };
        names.insert(key, name);
    }
    names
}

/// Where a found path is, against the matter's root.
enum Below {
    /// Not below the root.
    Outside,
    /// Below it, by these parts.
    Parts(Vec<String>),
    /// Below it, but a part isn't text, so it can't be recorded: it refuses, naming it.
    NotText,
}

/// Where `path` is against `root`.
fn below(root: &Path, path: &Path) -> Below {
    let Ok(rest) = path.strip_prefix(root) else {
        return Below::Outside;
    };
    let mut parts = Vec::new();
    for part in rest.components() {
        let Component::Normal(part) = part else {
            return Below::Outside;
        };
        let Some(text) = part.to_str() else {
            return Below::NotText;
        };
        parts.push(text.to_owned());
    }
    if parts.is_empty() {
        Below::Outside
    } else {
        Below::Parts(parts)
    }
}

/// Sets aside each protected name that is there now with another file ID than `record` holds
/// for it, or that `record` lacks (or holds as absent). One whose ID can't be read for want of
/// permission is set aside as one the lawyer can't open; one already gone is passed over, and
/// so is what is inside a folder this pass set aside. Every name is tried; the first failure is
/// returned after, naming its item.
pub(super) fn check(
    volume: &impl Volume,
    place: &Place,
    seen: &Seen<'_>,
    record: &Record,
    now: SystemTime,
    moved: &mut Vec<Entry>,
) -> Result<(), Refused> {
    let mut first = None;
    let mut aside: Vec<PathBuf> = Vec::new();
    for (key, name) in names(volume, seen) {
        if aside.iter().any(|folder| name.path.starts_with(folder)) {
            continue;
        }
        let was = record.names.get(&key).copied().flatten();
        let reason = match name.now {
            Now::Absent => continue,
            Now::At(id) if was == Some(id) => continue,
            Now::At(_) if was.is_none() => Reason::New,
            Now::At(_) => Reason::Changed,
            Now::Shut => Reason::LawyerCantOpen,
            Now::Failed(failure) => {
                first.get_or_insert(Refused::at(failure, &name.path));
                continue;
            }
        };
        match set_aside(volume, place, &name.path, &name.within, reason, now) {
            Ok(entry) => {
                aside.push(name.path);
                moved.push(entry);
            }
            Err(refused) if refused.failure == SetAsideFailure::Gone => {
                tracing::debug!("a protected name went before it could be set aside");
            }
            Err(refused) => {
                first.get_or_insert(refused);
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

/// Each protected name now and its file ID, `None` for a name at the root that is absent (the
/// root's names are read afresh); a found name that `moved` set aside, or that was inside
/// one, is left out. A name whose ID can't be read refuses, naming it.
pub(super) fn snapshot(
    volume: &impl Volume,
    seen: &Seen<'_>,
    moved: &[Entry],
) -> Result<BTreeMap<String, Option<FileId>>, Refused> {
    let mut record = BTreeMap::new();
    for (key, name) in names(volume, seen) {
        if !name.at_root && moved.iter().any(|entry| name.path.starts_with(&entry.from)) {
            continue;
        }
        let id = match name.now {
            Now::Absent => None,
            Now::At(id) => Some(id),
            Now::Shut => {
                let failure = SetAsideFailure::Io(io::ErrorKind::PermissionDenied);
                return Err(Refused::at(failure, &name.path));
            }
            Now::Failed(failure) => return Err(Refused::at(failure, &name.path)),
        };
        record.insert(key, id);
    }
    Ok(record)
}
