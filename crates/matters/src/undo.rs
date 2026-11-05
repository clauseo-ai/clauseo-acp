//! Undoing a step: put back, file by file, what the step between two checkpoints changed.
//! Everything moves forward: the result is a new `undo` checkpoint, so an undo can be undone.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use clauseo_protocol::{
    ChangeKind, CheckpointId, CheckpointKind, ContentId, FileChoice, MatterPath,
};

use crate::checkpoint::{Core, SizedFile, Spec};
use crate::diff::content_id;
use crate::history::{HistoryStore, Oid};
use crate::no_download::NoDownload;
use crate::stat::{CacheEntry, EntryState, Stat, find_by_path};
use crate::types::{Checkpoint, CheckpointOutcome, Settings, TimelineEntry, UndoRequest, Undone};
use crate::writeback::{self, Names, Op, skipped, writable};
use crate::{Error, locks};

/// One changed file of the step, by the path decisions name, and the operations undoing it.
struct Step {
    path: MatterPath,
    ops: Vec<Op>,
}

/// The step from `before` to `after` as undo operations, sorted by path. A deletion whose
/// file is there now but over the cap or cloud-only is left out: the checkpoint that saw it
/// leave the tree reported it as skipped, not deleted, and undo never writes such a file.
fn step(
    core: &Core,
    folder: &Path,
    settings: &Settings,
    before: CheckpointId,
    after: CheckpointId,
) -> Result<Vec<Step>, Error> {
    let mut changes = core.changes_between(before, after)?;
    let mut kept = Vec::with_capacity(changes.len());
    let mut names = Names::default();
    for change in changes.drain(..) {
        if matches!(change.change(), ChangeKind::Deleted)
            && now_skipped(&mut names, folder, settings, change.path())?
        {
            continue;
        }
        kept.push(change);
    }
    let changes = kept;
    let before_files = core.files_of(before)?;
    let blob = |path: &MatterPath| {
        find(&before_files, path.as_str())
            .map(|file| file.blob)
            .ok_or_else(|| Error::history("undo", "a changed file is missing from its tree"))
    };
    let mut steps = Vec::with_capacity(changes.len());
    for change in changes {
        let path = change.path().clone();
        let ops = match change.change() {
            ChangeKind::Added => vec![Op::Delete(path.clone())],
            ChangeKind::Deleted | ChangeKind::Modified => {
                vec![Op::Write(path.clone(), blob(&path)?)]
            }
            ChangeKind::Renamed { from } => {
                vec![
                    Op::Delete(path.clone()),
                    Op::Write(from.clone(), blob(from)?),
                ]
            }
            ChangeKind::Unknown { .. } => {
                return Err(Error::history("undo", "a change of an unknown kind"));
            }
        };
        steps.push(Step { path, ops });
    }
    Ok(steps)
}

fn find<'a>(files: &'a [SizedFile], path: &str) -> Option<&'a SizedFile> {
    find_by_path(files, path, |file| file.path.as_str())
}

/// What is at a path in the folder now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Current {
    Missing,
    Blob(Oid),
    /// Not a regular file, cloud-only or over the cap: never read.
    Unread,
}

/// See [`crate::Matter::edited_since`]. The paths an undo of the step would touch are those of
/// [`step`], each with its content in `after` as the step's diff reports it, so only the
/// subtrees the step changed are read: never a listing of the whole matter.
pub(crate) fn edited_since(
    core: &mut Core,
    folder: &Path,
    settings: &Settings,
    before: CheckpointId,
    after: CheckpointId,
) -> Result<Vec<MatterPath>, Error> {
    let changes = core.changes_between(before, after)?;
    let mut edited = Vec::new();
    let mut names = Names::default();
    let guard = NoDownload::begin(settings.probe.as_ref());
    for change in &changes {
        let path = change.path();
        // Each path undoing the change writes or deletes, with what `after` holds there.
        let targets: Vec<(&MatterPath, Option<&ContentId>)> = match change.change() {
            ChangeKind::Added | ChangeKind::Modified => vec![(path, change.after())],
            // As in `step`: a deletion whose file is back but over the cap or cloud-only is
            // never undone.
            ChangeKind::Deleted if now_skipped(&mut names, folder, settings, path)? => continue,
            ChangeKind::Deleted => vec![(path, None)],
            ChangeKind::Renamed { from } => vec![(path, change.after()), (from, None)],
            ChangeKind::Unknown { .. } => {
                return Err(Error::history("undo", "a change of an unknown kind"));
            }
        };
        for (target, then) in targets {
            let now = current(core, &mut names, folder, settings, target, &guard)?;
            let same = match (now, then) {
                (Current::Missing, None) | (Current::Unread, _) => true,
                (Current::Blob(now), Some(then)) => content_id(now)? == *then,
                (Current::Missing, Some(_)) | (Current::Blob(_), None) => false,
            };
            if !same {
                edited.push(path.clone());
                break;
            }
        }
    }
    Ok(edited)
}

/// The content at `path` now, hashing (without storing) only what the cache cannot vouch for.
/// A file found not downloaded when hashed, or behind a folder that does not list because it
/// is not downloaded, is [`Current::Unread`], as one the stat found cloud-only.
fn current(
    core: &mut Core,
    names: &mut Names,
    folder: &Path,
    settings: &Settings,
    path: &MatterPath,
    guard: &NoDownload<'_>,
) -> Result<Current, Error> {
    let abs = folder.join(path.as_str());
    let metadata = match names.metadata(folder, path) {
        Ok(Some(metadata)) => metadata,
        Ok(None) => return Ok(Current::Missing),
        Err(err) if err.is_not_downloaded() => return Ok(Current::Unread),
        Err(err) => return Err(err),
    };
    if skipped(settings, &abs, &metadata) {
        return Ok(Current::Unread);
    }
    let stat = Stat::from_metadata(&metadata);
    if let Some(blob) = core.cache.clean_blob(path.as_str(), &stat) {
        return Ok(Current::Blob(blob));
    }
    match core.history.hash_file(&abs, metadata.len(), guard) {
        Ok((blob, _)) => Ok(Current::Blob(blob)),
        Err(err) if err.is_not_downloaded() => Ok(Current::Unread),
        Err(err) => Err(err),
    }
}

/// Whether a regular file is at `path` now that is over the cap or cloud-only.
fn now_skipped(
    names: &mut Names,
    folder: &Path,
    settings: &Settings,
    path: &MatterPath,
) -> Result<bool, Error> {
    let abs = folder.join(path.as_str());
    Ok(names
        .metadata(folder, path)?
        .is_some_and(|metadata| metadata.is_file() && skipped(settings, &abs, &metadata)))
}

/// See [`crate::Matter::undo`].
pub(crate) fn undo(
    core: &mut Core,
    folder: &Path,
    settings: &Settings,
    request: &UndoRequest<'_>,
) -> Result<Undone, Error> {
    let steps = step(core, folder, settings, request.before, request.after)?;
    let choices = choices(&steps, request)?;
    let mut acting: Vec<&Step> = Vec::new();
    let mut kept: Vec<MatterPath> = Vec::new();
    for (step, choice) in steps.iter().zip(&choices) {
        let mut ok = *choice == FileChoice::Undo;
        if ok {
            for op in &step.ops {
                ok = ok && writable(folder, settings, op.path())?;
            }
        }
        if ok {
            acting.push(step);
        } else {
            kept.push(step.path.clone());
        }
    }
    keep_respelled(folder, &mut acting, &mut kept)?;
    let targets: Vec<MatterPath> = acting
        .iter()
        .flat_map(|step| step.ops.iter().map(|op| op.path().clone()))
        .collect();
    let locked = locks::locked(folder, &targets);
    if !locked.is_empty() {
        return Err(Error::FileLocked { paths: locked });
    }
    let outside = core.take(
        folder,
        settings,
        &spec(
            request.outside_edits,
            CheckpointKind::OutsideEdits,
            None,
            request,
        ),
    )?;
    let outside_edits = match outside.outcome {
        CheckpointOutcome::Created(_) => Some(outside),
        CheckpointOutcome::Reused(_) => None,
    };
    // A target that the outside-edits walk found cloud-only or over the cap (it went so after
    // `writable` looked at it, or was found not downloaded when read) is never written or
    // deleted: its step is kept. Nothing is written yet.
    acting.retain(|step| {
        let ok = step.ops.iter().all(|op| {
            core.cache
                .get(op.path().as_str())
                .is_none_or(|entry| matches!(entry.state, EntryState::Read(_)))
        });
        if !ok {
            kept.push(step.path.clone());
        }
        ok
    });
    // A step kept here can leave a file that another step's write would now replace (a delete
    // of `memo.txt` kept, a write of `Memo.txt` acting): check the names again.
    keep_respelled(folder, &mut acting, &mut kept)?;
    let after_files = core.files_of(request.after)?;
    let mut edited_since = Vec::new();
    for step in &acting {
        let edited = step.ops.iter().any(|op| {
            let then = find(&after_files, op.path().as_str()).map(|file| file.blob);
            let now = core
                .cache
                .get(op.path().as_str())
                .and_then(|entry| match entry.state {
                    EntryState::Read(blob) => Some(blob),
                    EntryState::Unread(_) | EntryState::Skipped => None,
                });
            now != then
        });
        if edited {
            edited_since.push(step.path.clone());
        }
    }
    let ops: Vec<&Op> = acting.iter().flat_map(|step| &step.ops).collect();
    let deleted = writeback::apply(core, folder, &ops)?;
    let (checkpoint, resurrected) = take_after_deletes(
        core,
        folder,
        settings,
        &spec(
            request.checkpoint,
            CheckpointKind::Undo,
            request.turn,
            request,
        ),
        &deleted,
    )?;
    let restored = acting.iter().map(|step| step.path.clone()).collect();
    kept.sort();
    Ok(Undone {
        outside_edits,
        checkpoint,
        restored,
        kept,
        edited_since,
        resurrected,
    })
}

/// Takes the `undo` checkpoint after `writeback::apply` deleted `deleted`, walking without the
/// cache's entries for those paths; the commit compares with the cache as it was. A sync client
/// can put a just-deleted file back as a cloud-only placeholder (`OneDrive` on macOS, about 3 s
/// later): found with its old entry, the walk would keep the old blob as `Unread` and the
/// undo's own checkpoint would hold the file it deleted. Without the entry it is skipped, or,
/// back and downloaded, read as what the folder holds. Also returns the paths of `deleted` the
/// walk found there again, sorted by path bytes.
fn take_after_deletes(
    core: &mut Core,
    folder: &Path,
    settings: &Settings,
    spec: &Spec<'_>,
    deleted: &[MatterPath],
) -> Result<(Checkpoint, Vec<MatterPath>), Error> {
    if deleted.is_empty() {
        return Ok((core.take(folder, settings, spec)?, Vec::new()));
    }
    // Before setting entries aside: a cache rebuilt from the tip would bring them back.
    core.check_cache()?;
    let gone: HashSet<&str> = deleted.iter().map(MatterPath::as_str).collect();
    let (aside, kept): (Vec<CacheEntry>, Vec<CacheEntry>) = std::mem::take(&mut core.cache.entries)
        .into_iter()
        .partition(|entry| gone.contains(entry.path.as_str()));
    core.cache.entries = kept;
    let scanned = core.scan_folder(folder, settings, None, &mut |_| {});
    if !aside.is_empty() {
        core.cache.entries.extend(aside);
        core.cache
            .entries
            .sort_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
    }
    let walked =
        scanned?.ok_or_else(|| Error::history("walk", "cancelled without a cancel flag"))?;
    let mut resurrected: Vec<MatterPath> = deleted
        .iter()
        .filter(|path| find_by_path(walked.entries(), path.as_str(), |e| e.path.as_str()).is_some())
        .cloned()
        .collect();
    resurrected.sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    resurrected.dedup();
    Ok((core.commit(walked, spec)?, resurrected))
}

/// The checkpoint spec for an undo's `outside-edits` (no thread or turn) or `undo` checkpoint.
fn spec<'a>(
    id: CheckpointId,
    kind: CheckpointKind,
    turn: Option<clauseo_protocol::TurnId>,
    request: &UndoRequest<'_>,
) -> Spec<'a> {
    let thread = if kind == CheckpointKind::Undo {
        request.thread
    } else {
        None
    };
    Spec {
        entry: TimelineEntry {
            id,
            kind,
            thread,
            turn,
        },
        at: request.at,
        tool_paths: None,
    }
}

/// Each step's choice, in step order: every step needs exactly one decision, and every
/// decision names a step.
fn choices(steps: &[Step], request: &UndoRequest<'_>) -> Result<Vec<FileChoice>, Error> {
    let mut by_path: BTreeMap<&str, FileChoice> = BTreeMap::new();
    for decision in request.decisions {
        let known = steps.iter().any(|step| step.path == decision.path);
        if !known
            || by_path
                .insert(decision.path.as_str(), decision.choice)
                .is_some()
        {
            return Err(Error::UnexpectedDecision {
                path: decision.path.clone(),
            });
        }
    }
    steps
        .iter()
        .map(|step| {
            by_path
                .get(step.path.as_str())
                .copied()
                .ok_or_else(|| Error::MissingDecision {
                    path: step.path.clone(),
                })
        })
        .collect()
}

/// Moves to `kept` each acting step that touches a file the folder holds only spelled
/// otherwise ([`writeback::respelled`]): writing would replace an unrelated file, such as a
/// `memo.txt` made after the step when the undo puts back `Memo.txt`, and deleting would
/// remove one, such as a `Memo.txt` made after the lawyer deleted the `memo.txt` the step
/// added. Undoing a case-only rename deletes the file it finds, so that undo goes ahead.
/// Repeated until nothing moves, since a kept step deletes nothing.
fn keep_respelled(
    folder: &Path,
    acting: &mut Vec<&Step>,
    kept: &mut Vec<MatterPath>,
) -> Result<(), Error> {
    let mut names = Names::default();
    loop {
        let ops: Vec<&Op> = acting.iter().flat_map(|step| &step.ops).collect();
        let clashes = writeback::respelled(&mut names, folder, &ops)?;
        if clashes.is_empty() {
            return Ok(());
        }
        acting.retain(|step| {
            let clash = step.ops.iter().any(|op| clashes.contains(op.path()));
            if clash {
                kept.push(step.path.clone());
            }
            !clash
        });
    }
}

#[cfg(test)]
mod tests;
