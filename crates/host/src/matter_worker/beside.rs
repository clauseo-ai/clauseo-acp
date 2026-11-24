//! Turns of one matter running side by side, on the matter's thread. What a running turn
//! "names" is every place its tool calls name as one they write, whatever their status
//! ([`TurnPaths::named`]: a call still pending or in progress may be writing it now), and the
//! files its completed shell commands wrote ([`super::writes_in`]).
//!
//! - A before-turn checkpoint taken while other turns run leaves the papers they name out of
//!   its outside edits: those are their work, not the lawyer's edits outside Clauseo. The
//!   checkpoint itself holds every file as the folder has it.
//! - The after-turn checkpoint of a turn that ran beside others ([`TurnOverlap`]) is measured
//!   from its own before-turn checkpoint, and a paper only the others named is left out of its
//!   card, one it wrote too kept with `also_changed_by` (`clauseo_matters::Overlap`). A paper
//!   only the cards of those that ended credit is left out only while it holds what their card
//!   left it with.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use clauseo_protocol::{ChangeKind, CheckpointId, ContentId, FileChange, MatterPath, ThreadId};

use crate::sent::TurnPaths;

/// What the after-turn checkpoint of a turn that ran beside others of its matter needs.
#[derive(Debug)]
pub(crate) struct TurnOverlap {
    /// The turn's own before-turn checkpoint.
    pub(crate) since: CheckpointId,
    /// The other turns of the matter that ran beside it and run still, or ended with no card,
    /// with their tool calls' places.
    pub(crate) running: Vec<(ThreadId, TurnPaths)>,
    /// The papers the cards of turns that ran beside it and ended meanwhile credit.
    pub(crate) ended: BTreeMap<MatterPath, Ended>,
}

/// A paper the cards of turns that ended credit.
#[derive(Debug, Clone, Default)]
pub(crate) struct Ended {
    /// Those turns' threads.
    pub(crate) threads: BTreeSet<ThreadId>,
    /// The content the last of those cards left it with; `None` for a deletion.
    pub(crate) left: Option<ContentId>,
}

/// What [`others`] makes of a [`TurnOverlap`] for `clauseo_matters::Overlap`.
pub(super) struct Others {
    /// Each paper the other turns named or were credited, with their threads.
    pub(super) others: BTreeMap<MatterPath, BTreeSet<ThreadId>>,
    /// The papers of `others` only the cards of turns that ended credit, with what they left.
    pub(super) left: BTreeMap<MatterPath, Option<ContentId>>,
}

/// What the start-up repair of a turn left open, which started at checkpoint `before`, needs of
/// the other turns its matter left open (`open`, with their tool calls' places): `None` when
/// there are none, or its start is not known.
pub(super) fn of_open(
    folder: &Path,
    before: Option<CheckpointId>,
    mut open: Vec<(ThreadId, TurnPaths)>,
) -> Option<(CheckpointId, Others)> {
    let since = before.filter(|_| !open.is_empty())?;
    for (_, paths) in &mut open {
        paths.resolve_start();
    }
    let overlap = TurnOverlap {
        since,
        running: open,
        ended: BTreeMap::new(),
    };
    Some((since, others(folder, &overlap)))
}

/// The papers `paths`, a running turn's, names in `folder`.
fn named(folder: &Path, paths: &TurnPaths) -> BTreeSet<MatterPath> {
    let mut named = super::places_in(folder, paths.named());
    let writes = super::writes_in(folder, &paths.shell_writes());
    named.extend(writes.iter().filter_map(|path| MatterPath::new(path).ok()));
    named
}

/// The papers the other turns of `overlap` named or were credited, with their threads, and
/// what the cards of those that ended left on the papers no running one names.
pub(super) fn others(folder: &Path, overlap: &TurnOverlap) -> Others {
    let mut others: BTreeMap<MatterPath, BTreeSet<ThreadId>> = BTreeMap::new();
    let mut left = BTreeMap::new();
    for (path, ended) in &overlap.ended {
        others.insert(path.clone(), ended.threads.clone());
        left.insert(path.clone(), ended.left.clone());
    }
    for (thread, paths) in &overlap.running {
        for path in named(folder, paths) {
            left.remove(&path);
            others.entry(path).or_default().insert(*thread);
        }
    }
    Others { others, left }
}

/// Drops from `changes` each change to a paper one of `running` names in `folder`; a rename
/// goes when either of its paths is named.
pub(super) fn leave_out(folder: &Path, running: &[TurnPaths], changes: &mut Vec<FileChange>) {
    if running.is_empty() || changes.is_empty() {
        return;
    }
    let named: BTreeSet<MatterPath> = running
        .iter()
        .flat_map(|paths| named(folder, paths))
        .collect();
    changes.retain(|change| {
        let from = match change.change() {
            ChangeKind::Renamed { from } => Some(from),
            ChangeKind::Added
            | ChangeKind::Modified
            | ChangeKind::Deleted
            | ChangeKind::Unknown { .. } => None,
        };
        !named.contains(change.path()) && from.is_none_or(|from| !named.contains(from))
    });
}
