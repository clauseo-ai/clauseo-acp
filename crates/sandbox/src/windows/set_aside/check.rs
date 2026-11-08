//! The check by change: each protected name and link the scan found, compared with the record,
//! and what is new or changed set aside, each at its own path below the matter's root (N1, R2).

use std::io;
use std::time::SystemTime;

use super::files::{FileId, Place, Record, Snapshot};
use super::moves::set_aside;
use super::names::Key;
use super::run::{Refused, Volume};
use super::scan::{Item, Kept, Now, Scan, inside, same_key};

/// An item this pass set aside, or found gone: its key, and its file ID when the scan read
/// one.
#[derive(Debug, Clone)]
pub(super) struct Aside {
    key: Key,
    id: Option<FileId>,
}

impl Aside {
    fn of(item: &Item) -> Aside {
        let id = match item.now {
            Now::At(id) => Some(id),
            Now::Absent | Now::Shut | Now::Failed(_) => None,
        };
        Aside {
            key: item.key.clone(),
            id,
        }
    }
}

/// Whether `item` went with something in `aside`: it is that item, by its exact key, or the
/// scan reached it through that item's file ID (a folder listed as `.GIT` that the `.git`
/// probe opened, say). Never by a key that only matches ignoring case: a case-sensitive
/// folder may hold `a\.git` beside `A\.git`, and that one is checked on its own (round 4).
fn gone_with(item: &Item, aside: &[Aside]) -> bool {
    aside.iter().any(|went| {
        inside(&item.key, &went.key) || went.id.is_some_and(|id| item.through.contains(&id))
    })
}
use super::{Entry, Reason, SetAsideFailure};

/// What the record holds for `item`: its file ID at the last pass, or `None` when the record
/// lacks it (or holds it as absent). The key exactly as scanned is looked up first, then one
/// that matches as [`same_key`] matches, so of `A\.git` and `a\.git` in a case-sensitive
/// folder each finds its own.
fn recorded(record: &Record, item: &Item) -> Option<FileId> {
    fn find<T: Copy>(map: &std::collections::BTreeMap<Key, T>, key: &Key) -> Option<T> {
        map.get(key).copied().or_else(|| {
            map.iter()
                .find(|(recorded, _)| same_key(recorded, key))
                .map(|(_, id)| *id)
        })
    }
    match item.kept {
        Kept::Root | Kept::Nested => find(&record.names, &item.key).flatten(),
        Kept::Link => find(&record.links, &item.key),
    }
}

/// Sets aside each protected name and link the scan found there now with another file ID than
/// `record` holds for it, or that `record` lacks (or holds as absent). One whose ID can't be
/// read for want of permission is set aside as one the lawyer can't open; one already gone is
/// passed over, and so is what is inside a folder this pass set aside (by its exact key or its
/// file ID: [`gone_with`]). A link is moved as itself, never followed. Every item is tried;
/// the first failure is returned after, naming its item. Returns the items set aside, or gone
/// before they could be, for [`snapshot`].
pub(super) fn check(
    volume: &impl Volume,
    place: &Place,
    scan: &Scan,
    record: &Record,
    now: SystemTime,
    moved: &mut Vec<Entry>,
) -> Result<Vec<Aside>, Refused> {
    let mut first = None;
    let mut aside: Vec<Aside> = Vec::new();
    for item in &scan.items {
        if gone_with(item, &aside) {
            continue;
        }
        let was = recorded(record, item);
        let reason = match item.now {
            Now::Absent => continue,
            Now::At(id) if was == Some(id) => continue,
            Now::At(_) if was.is_none() => Reason::New,
            Now::At(_) => Reason::Changed,
            Now::Shut => Reason::LawyerCantOpen,
            Now::Failed(failure) => {
                first.get_or_insert(Refused::at(failure, &item.path));
                continue;
            }
        };
        match set_aside(volume, place, &item.path, &item.within, reason, now) {
            Ok(entry) => {
                aside.push(Aside::of(item));
                moved.push(entry);
            }
            Err(refused) if refused.failure == SetAsideFailure::Gone => {
                tracing::debug!("a protected name or link went before it could be set aside");
                aside.push(Aside::of(item));
            }
            Err(refused) => {
                first.get_or_insert(refused);
            }
        }
    }
    if !moved.is_empty() {
        tracing::info!(
            count = moved.len(),
            "changed protected names and links were set aside"
        );
    }
    first.map_or(Ok(aside), Err)
}

/// What the record holds after a pass: each item the scan found and its file ID, with `None`
/// for a name at the root that is absent, was set aside or went; any other item set aside or
/// gone (`aside`), or inside one, is left out. A name in a folder the scan couldn't list isn't
/// recorded, so the next pass treats it as new. An item whose ID couldn't be read refuses,
/// naming it.
pub(super) fn snapshot(scan: &Scan, aside: &[Aside]) -> Result<Snapshot, Refused> {
    let mut seen = Snapshot::default();
    for item in &scan.items {
        let gone = gone_with(item, aside);
        match (item.kept, gone, item.now) {
            (Kept::Root, true, _) | (Kept::Root, false, Now::Absent) => {
                seen.names.insert(item.key.clone(), None);
            }
            (_, true, _) | (_, false, Now::Absent) => {}
            (Kept::Root | Kept::Nested, false, Now::At(id)) => {
                seen.names.insert(item.key.clone(), Some(id));
            }
            (Kept::Link, false, Now::At(id)) => {
                seen.links.insert(item.key.clone(), id);
            }
            (_, false, Now::Shut) => {
                let failure = SetAsideFailure::Io(io::ErrorKind::PermissionDenied);
                return Err(Refused::at(failure, &item.path));
            }
            (_, false, Now::Failed(failure)) => return Err(Refused::at(failure, &item.path)),
        }
    }
    Ok(seen)
}
