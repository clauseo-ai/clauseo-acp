//! Undoing a step: put back, file by file, what the step between two checkpoints changed.
//! Everything moves forward: the result is a new `undo` checkpoint, so an undo can be undone.

use std::collections::BTreeMap;
use std::fs::Metadata;
use std::path::{Path, PathBuf};

use clauseo_protocol::{ChangeKind, CheckpointId, CheckpointKind, FileChoice, MatterPath};

use crate::checkpoint::{Core, SizedFile, Spec};
use crate::history::{HistoryStore, Oid};
use crate::stat::{EntryState, Stat, find_by_path};
use crate::types::{CheckpointOutcome, Settings, TimelineEntry, UndoRequest, Undone};
use crate::writeback::{self, Names, Op, same_entry, skipped, writable};
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

/// See [`crate::Matter::edited_since`].
pub(crate) fn edited_since(
    core: &mut Core,
    folder: &Path,
    settings: &Settings,
    before: CheckpointId,
    after: CheckpointId,
) -> Result<Vec<MatterPath>, Error> {
    let steps = step(core, folder, settings, before, after)?;
    let after_files = core.files_of(after)?;
    let mut edited = Vec::new();
    let mut names = Names::default();
    for step in &steps {
        for op in &step.ops {
            let then = find(&after_files, op.path().as_str()).map(|file| file.blob);
            let now = current(core, &mut names, folder, settings, op.path())?;
            let same = match (now, then) {
                (Current::Missing, None) | (Current::Unread, _) => true,
                (Current::Blob(now), Some(then)) => now == then,
                (Current::Missing, Some(_)) | (Current::Blob(_), None) => false,
            };
            if !same {
                edited.push(step.path.clone());
                break;
            }
        }
    }
    Ok(edited)
}

/// The content at `path` now, hashing (without storing) only what the cache cannot vouch for.
fn current(
    core: &mut Core,
    names: &mut Names,
    folder: &Path,
    settings: &Settings,
    path: &MatterPath,
) -> Result<Current, Error> {
    let abs = folder.join(path.as_str());
    let Some(metadata) = names.metadata(folder, path)? else {
        return Ok(Current::Missing);
    };
    if skipped(settings, &abs, &metadata) {
        return Ok(Current::Unread);
    }
    let stat = Stat::from_metadata(&metadata);
    if let Some(blob) = core.cache.clean_blob(path.as_str(), &stat) {
        return Ok(Current::Blob(blob));
    }
    let (blob, _) = core.history.hash_file(&abs, metadata.len())?;
    Ok(Current::Blob(blob))
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
    writeback::apply(core, folder, &ops)?;
    let checkpoint = core.take(
        folder,
        settings,
        &spec(
            request.checkpoint,
            CheckpointKind::Undo,
            request.turn,
            request,
        ),
    )?;
    let restored = acting.iter().map(|step| step.path.clone()).collect();
    kept.sort();
    Ok(Undone {
        outside_edits,
        checkpoint,
        restored,
        kept,
        edited_since,
    })
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

/// Moves to `kept` each acting step that writes a path the folder holds only spelled otherwise
/// (another case or Unicode normalisation, on APFS or NTFS), unless that file is one this undo
/// deletes: writing it would replace an unrelated file, such as a `memo.txt` made after the step
/// when the undo puts back `Memo.txt`. Undoing a case-only rename deletes the file it finds, so
/// that undo goes ahead. Repeated until nothing moves, since a kept step deletes nothing.
fn keep_respelled(
    folder: &Path,
    acting: &mut Vec<&Step>,
    kept: &mut Vec<MatterPath>,
) -> Result<(), Error> {
    let mut names = Names::default();
    loop {
        let mut deleted: Vec<(PathBuf, Metadata)> = Vec::new();
        for op in acting.iter().flat_map(|step| &step.ops) {
            if let Op::Delete(path) = op
                && let Some(metadata) = names.metadata(folder, path)?
            {
                deleted.push((folder.join(path.as_str()), metadata));
            }
        }
        let before = acting.len();
        let mut still = Vec::with_capacity(before);
        for step in acting.drain(..) {
            let mut clash = false;
            for op in &step.ops {
                if let Op::Write(path, _) = op
                    && let Some(other) = names.other_spelling(folder, path)?
                {
                    let abs = folder.join(path.as_str());
                    let ours = deleted
                        .iter()
                        .any(|(gone, metadata)| same_entry(gone, metadata, &abs, &other));
                    clash = clash || !ours;
                }
            }
            if clash {
                kept.push(step.path.clone());
            } else {
                still.push(step);
            }
        }
        *acting = still;
        if acting.len() == before {
            return Ok(());
        }
    }
}
