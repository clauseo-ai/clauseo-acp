//! Restoring a version: put back the whole matter, or chosen files, as they were at a
//! checkpoint. Everything moves forward: the result is a new `restore` checkpoint, so a restore
//! can be undone. Files added since stay: a restore never deletes.
//!
//! The order, and what a crash between two steps leaves (nothing is committed or written before
//! step 4):
//!
//! 1. The request is checked: the ids, the chosen files, the version.
//! 2. One walk of the folder against the stat cache stores what it hashes, as any checkpoint
//!    does; nothing names those objects yet. A crash leaves unnamed loose objects that a later
//!    checkpoint finds and syncs.
//! 3. The plan (the version against that walk) and the checks of every target: links, names
//!    spelled otherwise, skipped files, locks. A lock fails the restore here.
//! 4. The walk is committed as the `outside-edits` checkpoint if the folder changed. A crash in
//!    its publish leaves the ref old or new, by decision record 0007's sync order.
//! 5. `writeback::apply`: stage, check the targets again, rename, then sync each folder once.
//!    A crash while staging leaves the folder as it was (with `.clauseo-<n>.tmp` files at
//!    worst); while renaming or before the folder syncs, partly restored, each file old or new,
//!    recorded by nothing until the next checkpoint, which takes it as outside edits. A retry
//!    with fresh ids writes only what still differs.
//! 6. The `restore` checkpoint. A crash before its publish leaves the folder restored and
//!    recorded by the next checkpoint as outside edits; a retry finds nothing to write.

use std::collections::BTreeSet;
use std::path::Path;

use clauseo_protocol::{ChangeKind, CheckpointId, CheckpointKind, Count, FileChange, MatterPath};

use crate::checkpoint::{Core, Spec, VersionEntry};
use crate::diff::content_id;
use crate::history::Oid;
use crate::stat::{CacheEntry, EntryState};
use crate::types::{
    Checkpoint, CheckpointOutcome, Restore, RestoreFiles, RestorePlan, Restored, Settings,
    TimelineEntry,
};
use crate::writeback::{self, Names, Op, writable};
use crate::{Error, locks, rules};

/// One file the restore writes: the version's blob, the blob there now (`None`: absent), and
/// the version's size.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Write {
    path: MatterPath,
    blob: Oid,
    now: Option<Oid>,
    size: u64,
}

/// A restore's plan, before the lock check.
#[derive(Debug, Default, PartialEq, Eq)]
struct Plan {
    writes: Vec<Write>,
    added_since: Vec<MatterPath>,
    kept: Vec<MatterPath>,
}

impl Plan {
    fn targets(&self) -> Vec<MatterPath> {
        self.writes.iter().map(|write| write.path.clone()).collect()
    }

    /// The public plan, with `locked`.
    fn public(self, locked: Vec<MatterPath>) -> Result<RestorePlan, Error> {
        let writes = self
            .writes
            .iter()
            .map(|write| {
                let after = Some(content_id(write.blob)?);
                let size = Count::clamped(write.size);
                let change = match write.now {
                    Some(now) => FileChange::new(
                        write.path.clone(),
                        ChangeKind::Modified,
                        Some(content_id(now)?),
                        after,
                        size,
                    ),
                    None => {
                        FileChange::new(write.path.clone(), ChangeKind::Added, None, after, size)
                    }
                };
                change.map_err(|err| Error::history("restore", err))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(RestorePlan {
            writes,
            added_since: self.added_since,
            kept: self.kept,
            locked,
        })
    }
}

/// A file the version and the folder disagree on, before the file system checks: the version's
/// blob and what is there now.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    path: MatterPath,
    blob: Oid,
    now: Option<Oid>,
}

/// What the version (`then`) and the folder (`now`, a walk's entries) disagree on, from the
/// lists alone, restricted to `chosen` when given (checked by [`chosen`]). `UnknownFile` for a
/// chosen path neither list has.
#[derive(Debug, Default, PartialEq, Eq)]
struct Draft {
    candidates: Vec<Candidate>,
    added_since: Vec<MatterPath>,
    kept: Vec<MatterPath>,
}

fn draft(
    then: &[VersionEntry],
    now: &[CacheEntry],
    chosen: Option<&BTreeSet<&str>>,
) -> Result<Draft, Error> {
    let in_scope = |path: &MatterPath| chosen.is_none_or(|chosen| chosen.contains(path.as_str()));
    let mut out = Draft::default();
    let mut seen = 0_usize;
    let (mut t_iter, mut n_iter) = (then.iter().peekable(), now.iter().peekable());
    loop {
        let (t, n) = match (t_iter.peek(), n_iter.peek()) {
            (None, None) => break,
            (Some(t), Some(n)) if t.path.as_str() == n.path.as_str() => {
                let pair = (Some(*t), Some(*n));
                t_iter.next();
                n_iter.next();
                pair
            }
            (Some(t), Some(n)) if t.path.as_str().as_bytes() < n.path.as_str().as_bytes() => {
                let pair = (Some(*t), None);
                t_iter.next();
                pair
            }
            (Some(_) | None, Some(n)) => {
                let pair = (None, Some(*n));
                n_iter.next();
                pair
            }
            (Some(t), None) => {
                let pair = (Some(*t), None);
                t_iter.next();
                pair
            }
        };
        let Some(path) = t.map(|t| &t.path).or(n.map(|n| &n.path)) else {
            break;
        };
        if !in_scope(path) {
            continue;
        }
        seen += 1;
        match (t, n.map(|n| n.state)) {
            (Some(t), Some(EntryState::Read(blob))) if blob == t.blob => {}
            (Some(t), Some(EntryState::Read(blob))) => out.candidates.push(Candidate {
                path: t.path.clone(),
                blob: t.blob,
                now: Some(blob),
            }),
            // Cloud-only now: the same content stays; other content is never written over a
            // placeholder.
            (Some(t), Some(EntryState::Unread(blob))) if blob == t.blob => {}
            (Some(_), Some(EntryState::Unread(_) | EntryState::Skipped)) => {
                out.kept.push(path.clone());
            }
            (Some(t), None) => out.candidates.push(Candidate {
                path: t.path.clone(),
                blob: t.blob,
                now: None,
            }),
            (None, Some(EntryState::Read(_) | EntryState::Unread(_))) => {
                out.added_since.push(path.clone());
            }
            // Over the cap or cloud-only and never versioned: listed only when chosen.
            (None, Some(EntryState::Skipped)) => {
                if chosen.is_some() {
                    out.kept.push(path.clone());
                }
            }
            (None, None) => {}
        }
    }
    if let Some(chosen) = chosen
        && seen < chosen.len()
    {
        let known: BTreeSet<&str> = then
            .iter()
            .map(|file| file.path.as_str())
            .chain(now.iter().map(|entry| entry.path.as_str()))
            .filter(|path| chosen.contains(path))
            .collect();
        if let Some(missing) = chosen.iter().find(|path| !known.contains(**path)) {
            let path = MatterPath::new(missing).map_err(|err| Error::history("restore", err))?;
            return Err(Error::UnknownFile { path });
        }
    }
    Ok(out)
}

/// The chosen paths of `files` as a set; `None` for every file. `NoFilesChosen` for an empty
/// list, `UnknownFile` for a path named twice.
fn chosen(files: RestoreFiles<'_>) -> Result<Option<BTreeSet<&str>>, Error> {
    let RestoreFiles::Only(paths) = files else {
        return Ok(None);
    };
    if paths.is_empty() {
        return Err(Error::NoFilesChosen);
    }
    let mut set = BTreeSet::new();
    for path in paths {
        if !set.insert(path.as_str()) {
            return Err(Error::UnknownFile { path: path.clone() });
        }
    }
    Ok(Some(set))
}

/// The plan: [`draft`], then each candidate checked against the file system. A candidate is
/// kept, never written, when its name is excluded now, its version is larger than today's cap,
/// [`writable`] refuses it, or the folder holds it only spelled otherwise (writing would
/// replace that other file).
fn plan(
    core: &Core,
    folder: &Path,
    settings: &Settings,
    then: &[VersionEntry],
    now: &[CacheEntry],
    chosen: Option<&BTreeSet<&str>>,
) -> Result<Plan, Error> {
    let draft = draft(then, now, chosen)?;
    let cap = settings.limits.effective_cap();
    let mut names = Names::default();
    let mut plan = Plan {
        writes: Vec::with_capacity(draft.candidates.len()),
        added_since: draft.added_since,
        kept: draft.kept,
    };
    for candidate in draft.candidates {
        let size = core.size_of(candidate.path.as_str(), candidate.blob)?;
        let ok = !rules::is_excluded_path(candidate.path.as_str())
            && size <= cap
            && writable(folder, settings, &candidate.path)?
            && names.other_spelling(folder, &candidate.path)?.is_none();
        if ok {
            plan.writes.push(Write {
                path: candidate.path,
                blob: candidate.blob,
                now: candidate.now,
                size,
            });
        } else {
            plan.kept.push(candidate.path);
        }
    }
    plan.kept
        .sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    Ok(plan)
}

/// See [`crate::Matter::preview_restore`].
pub(crate) fn preview(
    core: &Core,
    folder: &Path,
    settings: &Settings,
    from: CheckpointId,
    files: RestoreFiles<'_>,
) -> Result<RestorePlan, Error> {
    let chosen = chosen(files)?;
    let then = core.tree_files(from)?;
    let now = core.snapshot(folder, settings)?;
    let plan = plan(core, folder, settings, &then, &now, chosen.as_ref())?;
    let locked = locks::locked(folder, &plan.targets());
    plan.public(locked)
}

/// See [`crate::Matter::restore`]. The steps are the module's.
pub(crate) fn restore(
    core: &mut Core,
    folder: &Path,
    settings: &Settings,
    request: &Restore<'_>,
) -> Result<Restored, Error> {
    // 1. The request.
    let chosen = chosen(request.files)?;
    for id in [request.outside_edits, request.checkpoint] {
        if core.timeline.contains(id) {
            return Err(Error::DuplicateCheckpoint { id });
        }
    }
    if request.outside_edits == request.checkpoint {
        return Err(Error::DuplicateCheckpoint {
            id: request.checkpoint,
        });
    }
    let then = core.tree_files(request.from)?;
    // 2. One walk, whose stored objects the outside-edits checkpoint reuses.
    let walked = core
        .scan_folder(folder, settings, None, &mut |_| {})?
        .ok_or_else(|| Error::history("walk", "cancelled without a cancel flag"))?;
    // 3. The plan and every check before anything is committed.
    let plan = plan(
        core,
        folder,
        settings,
        &then,
        walked.entries(),
        chosen.as_ref(),
    )?;
    let locked = locks::locked(folder, &plan.targets());
    if !locked.is_empty() {
        return Err(Error::FileLocked { paths: locked });
    }
    // 4. The outside-edits checkpoint.
    let outside = core.commit(
        walked,
        &spec(request.outside_edits, CheckpointKind::OutsideEdits, request),
    )?;
    let outside_edits = match outside.outcome {
        CheckpointOutcome::Created(_) => Some(outside),
        CheckpointOutcome::Reused(_) => None,
    };
    let restored: Vec<MatterPath> = plan.targets();
    if restored.is_empty() {
        // Nothing to write: no second walk, so nothing edited meanwhile is called a restore.
        let (last, _, _) = core
            .timeline
            .head()
            .ok_or_else(|| Error::history("restore", "the timeline has no checkpoint"))?;
        return Ok(Restored {
            outside_edits,
            checkpoint: Checkpoint {
                outcome: CheckpointOutcome::Reused(last),
                changes: Vec::new(),
                skipped: Vec::new(),
            },
            written: restored,
            added_since: plan.added_since,
            kept: plan.kept,
        });
    }
    // 5. Stage, check again, rename.
    let ops: Vec<Op> = plan
        .writes
        .iter()
        .map(|write| Op::Write(write.path.clone(), write.blob))
        .collect();
    let ops: Vec<&Op> = ops.iter().collect();
    writeback::apply(core, folder, &ops)?;
    // 6. The restore checkpoint.
    let checkpoint = core.take(
        folder,
        settings,
        &spec(request.checkpoint, CheckpointKind::Restore, request),
    )?;
    Ok(Restored {
        outside_edits,
        checkpoint,
        written: restored,
        added_since: plan.added_since,
        kept: plan.kept,
    })
}

/// The spec of a restore's `outside-edits` or `restore` checkpoint: no thread, no turn.
fn spec<'a>(id: CheckpointId, kind: CheckpointKind, request: &Restore<'_>) -> Spec<'a> {
    Spec {
        entry: TimelineEntry {
            id,
            kind,
            thread: None,
            turn: None,
        },
        at: request.at,
        tool_paths: None,
    }
}

#[cfg(test)]
mod tests;
