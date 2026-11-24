//! Turns of one matter running side by side, on the matter's thread. A running turn's places
//! are read when the job runs, not when it was sent (`sent/shared.rs`), so a call that starts
//! in between counts.
//!
//! - A before-turn checkpoint taken while other turns run leaves the papers they name out of
//!   its outside edits: every place their tool calls name as one they write, whatever their
//!   status ([`TurnPaths::named`]: a call still pending or in progress may be writing it now),
//!   and the files their completed shell commands wrote ([`super::writes_in`]). Those are their
//!   work, not the lawyer's edits outside Clauseo. The checkpoint itself holds every file as the
//!   folder has it.
//! - The after-turn checkpoint of a turn that ran beside others ([`TurnOverlap`]) is measured
//!   from its own before-turn checkpoint (`clauseo_matters::Overlap`). A paper another turn
//!   changed (what its completed calls and shell commands wrote, or its card credits) is left
//!   out of the card unless the turn wrote it too, then kept with `also_changed_by`. One only an
//!   unfinished call of a turn still running names is left out the same way, but never named:
//!   a failed or refused call changed nothing. A paper only the cards of those that ended
//!   credit is left out only while it holds what their card left it with; one the turn itself
//!   names, or one a turn that ended with no card wrote, never is.
//! - The start-up repair ([`of_repair`]) measures an open turn the same way, the other open
//!   turns taken as running and the cards logged after its start as ended, and leaves out what
//!   checkpoints after its start logged as outside edits.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use clauseo_matters::Overlap;
use clauseo_protocol::{ChangeKind, CheckpointId, ContentId, FileChange, MatterPath, ThreadId};

use crate::sent::{SharedPaths, TurnPaths};

/// What the after-turn checkpoint of a turn that ran beside others of its matter needs.
#[derive(Debug)]
pub(crate) struct TurnOverlap {
    /// The turn's own before-turn checkpoint.
    pub(crate) since: CheckpointId,
    /// The other turns of the matter that ran beside it and run still, with their tool calls'
    /// places, read when the job runs.
    pub(crate) running: Vec<(ThreadId, SharedPaths)>,
    /// The turns that ran beside it and ended with no card (their after-turn checkpoint or its
    /// events failed), with their tool calls' places: what they wrote is never left out.
    pub(crate) failed: Vec<(ThreadId, TurnPaths)>,
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
pub(super) struct Beside {
    /// The turn's own before-turn checkpoint.
    since: CheckpointId,
    /// Each paper the other turns wrote or were credited, with their threads.
    others: BTreeMap<MatterPath, BTreeSet<ThreadId>>,
    /// Each paper only unfinished calls of the turns still running name, with their threads.
    busy: BTreeMap<MatterPath, BTreeSet<ThreadId>>,
    /// The papers of `others` only the cards of turns that ended credit, with what they left.
    left: BTreeMap<MatterPath, Option<ContentId>>,
    /// The papers checkpoints after `since` logged as outside edits or put on a card flagged,
    /// with the content logged (the start-up repair's; empty otherwise).
    logged: BTreeMap<MatterPath, Option<ContentId>>,
    /// Each paper the turns that ended with no card wrote, with their threads.
    failed: BTreeMap<MatterPath, BTreeSet<ThreadId>>,
    /// Each paper the turn itself names.
    own: BTreeSet<MatterPath>,
}

impl Beside {
    /// The overlap R8 measures the turn's after-turn checkpoint with.
    pub(super) fn overlap(&self) -> Overlap<'_> {
        Overlap {
            since: self.since,
            others: &self.others,
            busy: &self.busy,
            logged: &self.logged,
            left: &self.left,
            failed: &self.failed,
            own: &self.own,
        }
    }
}

/// A card or an outside-edits record the logs hold, for the start-up repair.
#[derive(Debug, Clone)]
pub(crate) struct Logged {
    /// The checkpoint it is of: an after-turn checkpoint for a card.
    pub(crate) checkpoint: CheckpointId,
    /// The answer whose card it is; `None` for outside edits.
    pub(crate) thread: Option<ThreadId>,
    /// Its files.
    pub(crate) files: Vec<FileChange>,
}

/// What the start-up repair of a turn left open, which started at checkpoint `since`, needs:
/// the other turns its matter left open (`open`, with their tool calls' places) taken as
/// running, the cards and outside edits logged after `since` (`later`, in timeline order):
/// a card's papers its answer wrote as an ended answer's, with the content it left, and the
/// others, flagged on a card or outside edits, as logged. `own` are the turn's own places.
pub(super) fn of_repair(
    folder: &Path,
    since: CheckpointId,
    open: Vec<(ThreadId, TurnPaths)>,
    later: &[&Logged],
    own: &TurnPaths,
) -> Beside {
    let mut ended: BTreeMap<MatterPath, Ended> = BTreeMap::new();
    let mut logged = BTreeMap::new();
    for record in later {
        for file in &record.files {
            let mut paths = vec![(file.path().clone(), file.after().cloned())];
            if let ChangeKind::Renamed { from } = file.change() {
                paths.push((from.clone(), None));
            }
            for (path, content) in paths {
                match record.thread {
                    Some(thread) if !file.may_include_lawyer_edits() => {
                        let entry = ended.entry(path).or_default();
                        entry.threads.insert(thread);
                        entry.left = content;
                    }
                    Some(_) | None => {
                        logged.insert(path, content);
                    }
                }
            }
        }
    }
    let running = open
        .into_iter()
        .map(|(thread, mut paths)| {
            paths.resolve_start();
            (thread, SharedPaths::new(paths))
        })
        .collect();
    let overlap = TurnOverlap {
        since,
        running,
        failed: Vec::new(),
        ended,
    };
    let mut beside = others(folder, &overlap, own);
    beside.logged = logged;
    beside
}

/// The papers `paths`, a running turn's, names in `folder` as ones it writes, whatever their
/// calls' status, with its completed shell commands' files.
fn named(folder: &Path, paths: &TurnPaths) -> BTreeSet<MatterPath> {
    let mut named = super::places_in(folder, paths.named());
    named.extend(shell_writes(folder, paths));
    named
}

/// The papers `paths` wrote in `folder`: its completed calls' places
/// ([`super::tool_paths`]) and its completed shell commands' files.
fn credited(folder: &Path, paths: &TurnPaths) -> BTreeSet<MatterPath> {
    let mut credited = super::tool_paths(folder, paths);
    credited.extend(shell_writes(folder, paths));
    credited
}

/// The files `paths`' completed shell commands wrote in `folder`.
fn shell_writes(folder: &Path, paths: &TurnPaths) -> impl Iterator<Item = MatterPath> {
    let writes = super::writes_in(folder, &paths.shell_writes());
    writes
        .into_iter()
        .filter_map(|path| MatterPath::new(&path).ok())
}

/// The papers the other turns of `overlap` wrote or were credited, with their threads, those
/// only unfinished calls of the running ones name, what the cards of those that ended left on
/// the papers no running one names, and the papers `own`, the turn's own places, names.
pub(super) fn others(folder: &Path, overlap: &TurnOverlap, own: &TurnPaths) -> Beside {
    let mut others: BTreeMap<MatterPath, BTreeSet<ThreadId>> = BTreeMap::new();
    let mut busy: BTreeMap<MatterPath, BTreeSet<ThreadId>> = BTreeMap::new();
    let mut left = BTreeMap::new();
    for (path, ended) in &overlap.ended {
        others.insert(path.clone(), ended.threads.clone());
        left.insert(path.clone(), ended.left.clone());
    }
    for (thread, shared) in &overlap.running {
        let paths = shared.snapshot();
        for path in credited(folder, &paths) {
            left.remove(&path);
            others.entry(path).or_default().insert(*thread);
        }
        for path in super::places_in(folder, paths.unfinished()) {
            left.remove(&path);
            busy.entry(path).or_default().insert(*thread);
        }
    }
    let mut failed: BTreeMap<MatterPath, BTreeSet<ThreadId>> = BTreeMap::new();
    for (thread, paths) in &overlap.failed {
        for path in credited(folder, paths) {
            failed.entry(path).or_default().insert(*thread);
        }
    }
    Beside {
        since: overlap.since,
        others,
        busy,
        left,
        logged: BTreeMap::new(),
        failed,
        own: named(folder, own),
    }
}

/// Drops from `changes` each change to a paper one of `running` names in `folder` as it is when
/// the job runs (a rename goes when either of its paths is named), and returns those dropped,
/// each naming in `also_changed_by` the turns that name it.
pub(super) fn leave_out(
    folder: &Path,
    running: &[(ThreadId, SharedPaths)],
    changes: &mut Vec<FileChange>,
) -> Vec<FileChange> {
    if running.is_empty() || changes.is_empty() {
        return Vec::new();
    }
    let mut named: BTreeMap<MatterPath, BTreeSet<ThreadId>> = BTreeMap::new();
    for (thread, paths) in running {
        for path in self::named(folder, &paths.snapshot()) {
            named.entry(path).or_default().insert(*thread);
        }
    }
    let mut dropped = Vec::new();
    for change in std::mem::take(changes) {
        let from = match change.change() {
            ChangeKind::Renamed { from } => Some(from),
            ChangeKind::Added
            | ChangeKind::Modified
            | ChangeKind::Deleted
            | ChangeKind::Unknown { .. } => None,
        };
        let threads: BTreeSet<ThreadId> = [Some(change.path()), from]
            .into_iter()
            .flatten()
            .filter_map(|path| named.get(path))
            .flatten()
            .copied()
            .collect();
        if threads.is_empty() {
            changes.push(change);
        } else {
            dropped.push(change.with_also_changed_by(threads.into_iter().collect()));
        }
    }
    dropped
}
