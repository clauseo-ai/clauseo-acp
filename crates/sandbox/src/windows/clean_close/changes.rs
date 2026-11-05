//! Reading the journal from the record's cursor, and what the records found mean for the walk.
//!
//! A record counts when its file reference is an item the record holds, or its parent
//! reference is a folder the record holds (or one a counted record brought in: a new folder's
//! later records count too). Its reason decides whether it can change a walk's result:
//! [`IGNORABLE`] reasons can't (data writes, closes, basic information, extended attributes,
//! object IDs, indexing, streams, compression, integrity); every other bit, an unknown one
//! included, makes the item changed.
//!
//! A changed item's folder (the record's parent when the record holds it, and each folder the
//! record holds the item in, for a name changed through a hard link elsewhere) gets a walk of its
//! children ([`Reach::Children`]); a changed folder that isn't a link (new, moved in, its own
//! descriptor changed) gets a walk of its whole subtree ([`Reach::Tree`]), since a folder moved
//! in brings its contents with no record of their own. A changed own root means the full walk:
//! a root's descriptor is what every walk starts from.
//!
//! **The threshold** is [`MOST_TOUCHED`] (2,000) changed items, or 5% of the trees' items if
//! that is fewer. Past it, the targeted walks cost about what the full walk does: each changed
//! item's folder is listed and its children worked out (about 0.1 ms each, S15's measurements),
//! and in the measured trees a folder holds about ten names, so 5% of the items changed means
//! walking about half the trees in pieces. The fixed cap also bounds what the check keeps in
//! memory while it reads (one entry per changed item).

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use super::journal::{Change, Flow, Journal, JournalError, JournalState};
use super::record::{Known, VolumeMark};
use super::{FullReason, MOST_TOUCHED, Target};
use crate::windows::admit::PathKind;
use crate::windows::writer::Reach;

/// `USN_REASON_*` bits that can't change what a walk or the set-aside does: `DATA_OVERWRITE`,
/// `DATA_EXTEND`, `DATA_TRUNCATION`, the three `NAMED_DATA_*`, `EA_CHANGE`,
/// `INDEXABLE_CHANGE`, `BASIC_INFO_CHANGE`, `COMPRESSION_CHANGE`, `OBJECT_ID_CHANGE`,
/// `STREAM_CHANGE`, `INTEGRITY_CHANGE` and `CLOSE`.
pub(super) const IGNORABLE: u32 = 0x1
    | 0x2
    | 0x4
    | 0x10
    | 0x20
    | 0x40
    | 0x400
    | 0x4000
    | 0x8000
    | 0x0002_0000
    | 0x0008_0000
    | 0x0020_0000
    | 0x0080_0000
    | 0x8000_0000;

/// The most changed items a check handles for trees holding `items` items.
pub(super) fn most_touched(items: usize) -> usize {
    MOST_TOUCHED.min(items / 20)
}

/// One volume's changed folders, by file reference.
#[derive(Debug, Default)]
pub(super) struct Touched {
    /// Folders whose names changed: their children are walked.
    pub(super) children: BTreeSet<u64>,
    /// Folders new or changed themselves: their subtrees are walked.
    pub(super) trees: BTreeSet<u64>,
}

/// An own root as the launch found it.
#[derive(Debug, Clone)]
pub(super) struct Live {
    /// The policy's path.
    pub(super) path: PathBuf,
    pub(super) kind: PathKind,
    pub(super) volume: u64,
    /// Its final path, the form [`Journal::path_of`] gives.
    pub(super) found: PathBuf,
}

/// Reads `mark`'s volume's records from its cursor up to `state.next`, through `root`, into
/// the folders to walk. `seen` holds the changed items so far (all volumes) and `most` is the
/// threshold.
pub(super) fn read_volume(
    journal: &dyn Journal,
    root: &Path,
    mark: &VolumeMark,
    state: JournalState,
    known: &Known,
    seen: &mut BTreeSet<(u64, u64)>,
    most: usize,
) -> Result<Touched, FullReason> {
    let mut touched = Touched::default();
    // Items a counted record brought in, and the folder it brought each into.
    let mut new: BTreeMap<u64, (u64, bool)> = BTreeMap::new();
    let mut stop = None;
    let mut each = |change: &Change| {
        match count(change, known, &mut new, &mut touched) {
            Ok(false) => {}
            Ok(true) => {
                seen.insert((mark.volume, change.file));
                if seen.len() > most {
                    stop = Some(FullReason::TooMany);
                }
            }
            Err(reason) => stop = Some(reason),
        }
        if stop.is_some() {
            Flow::Stop
        } else {
            Flow::More
        }
    };
    let read = journal.read(root, mark.journal, mark.cursor, state.next, &mut each);
    if let Some(reason) = stop {
        return Err(reason);
    }
    read.map_err(|error| match error {
        JournalError::NotNtfs => FullReason::NotNtfs,
        JournalError::NoJournal => FullReason::NoJournal,
        JournalError::Unsupported => FullReason::Unsupported,
        JournalError::Lost => FullReason::Wrapped,
        JournalError::Failed(_) => FullReason::ReadFailed,
    })?;
    Ok(touched)
}

/// What one record means: `Ok(true)` when it changed something the walks cover, after adding
/// the folders to walk; `Ok(false)` when it doesn't count.
fn count(
    change: &Change,
    known: &Known,
    new: &mut BTreeMap<u64, (u64, bool)>,
    touched: &mut Touched,
) -> Result<bool, FullReason> {
    if change.reason & !IGNORABLE == 0 {
        return Ok(false);
    }
    let file_known = known.get(change.file);
    let parent_known = known.folder(change.parent) || new.get(&change.parent).is_some_and(|n| n.1);
    let file_new = new.get(&change.file).copied();
    if file_known.is_empty() && !parent_known && file_new.is_none() {
        return Ok(false);
    }
    if known.root(change.file) {
        return Err(FullReason::RootTouched);
    }
    if parent_known {
        touched.children.insert(change.parent);
    }
    for item in file_known {
        touched.children.insert(item.parent);
    }
    if let Some((parent, _)) = file_new {
        touched.children.insert(parent);
    }
    if change.plain_folder() {
        touched.trees.insert(change.file);
    }
    if file_known.is_empty() && parent_known {
        new.insert(change.file, (change.parent, change.plain_folder()));
    }
    Ok(true)
}

/// The targets for `touched` on `volume`: each folder found by its ID, inside the own root on
/// that volume that holds it most closely. A folder gone, a link now, or outside every own root
/// is left out: nothing of it is in the trees. One whose place can't be read is
/// [`FullReason::Unresolved`].
pub(super) fn resolve(
    journal: &dyn Journal,
    live: &[Live],
    volume: u64,
    touched: &Touched,
) -> Result<Vec<Target>, FullReason> {
    let on: Vec<&Live> = live.iter().filter(|l| l.volume == volume).collect();
    let Some(first) = on.first() else {
        return Err(FullReason::Unresolved);
    };
    let mut targets = Vec::new();
    let wanted = touched
        .trees
        .iter()
        .map(|id| (*id, Reach::Tree))
        .chain(touched.children.iter().map(|id| (*id, Reach::Children)));
    for (id, reach) in wanted {
        let found = journal
            .path_of(&first.path, volume, id)
            .map_err(|_unread| FullReason::Unresolved)?;
        let Some(found) = found.filter(|f| !f.link) else {
            continue;
        };
        let holder = on
            .iter()
            .filter_map(|l| found.path.strip_prefix(&l.found).ok().map(|rest| (l, rest)))
            .max_by_key(|(l, _)| l.found.components().count());
        let Some((root, rest)) = holder else {
            continue;
        };
        let parts = rest
            .components()
            .map(|c| match c {
                Component::Normal(part) => Ok(part.to_owned()),
                Component::Prefix(_)
                | Component::RootDir
                | Component::CurDir
                | Component::ParentDir => Err(FullReason::Unresolved),
            })
            .collect::<Result<Vec<OsString>, FullReason>>()?;
        targets.push(Target {
            root: root.path.clone(),
            kind: root.kind,
            parts,
            reach,
        });
    }
    Ok(targets)
}

/// `targets` without repeats, and without any a subtree walk covers: one strictly inside a
/// [`Reach::Tree`] target, or a children walk of a folder whose subtree is walked. Shallowest
/// first.
pub(super) fn reduce(mut targets: Vec<Target>) -> Vec<Target> {
    let trees: Vec<PathBuf> = targets
        .iter()
        .filter(|t| t.reach == Reach::Tree)
        .map(Target::path)
        .collect();
    targets.retain(|t| {
        let path = t.path();
        !trees
            .iter()
            .any(|tree| path.starts_with(tree) && (path != *tree || t.reach == Reach::Children))
    });
    targets.sort_by(|a, b| {
        let key = |t: &Target| {
            (
                t.path().components().count(),
                t.path(),
                t.reach == Reach::Tree,
            )
        };
        key(a).cmp(&key(b))
    });
    targets.dedup();
    targets
}
