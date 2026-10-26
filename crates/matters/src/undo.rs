//! Undoing a step: put back, file by file, what the step between two checkpoints changed.
//! Everything moves forward: the result is a new `undo` checkpoint, so an undo can be undone.

use std::collections::BTreeMap;
use std::fs::Metadata;
use std::io;
use std::path::Path;

use clauseo_protocol::{ChangeKind, CheckpointId, CheckpointKind, FileChoice, MatterPath};

use crate::checkpoint::{Core, SizedFile, Spec};
use crate::history::{HistoryStore, Oid};
use crate::stat::{EntryState, Stat};
use crate::types::{CheckpointOutcome, Settings, TimelineEntry, UndoRequest, Undone};
use crate::{Error, locks, write};

/// One file operation that puts a path back as it was before the step.
#[derive(Debug, Clone)]
enum Op {
    /// Write this blob to the path.
    Write(MatterPath, Oid),
    /// Delete the path (the step created it).
    Delete(MatterPath),
}

impl Op {
    const fn path(&self) -> &MatterPath {
        match self {
            Self::Write(path, _) | Self::Delete(path) => path,
        }
    }
}

/// One changed file of the step, by the path decisions name, and the operations undoing it.
struct Step {
    path: MatterPath,
    ops: Vec<Op>,
}

/// The step from `before` to `after` as undo operations, sorted by path.
fn step(core: &Core, before: CheckpointId, after: CheckpointId) -> Result<Vec<Step>, Error> {
    let changes = core.changes_between(before, after)?;
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
    files
        .binary_search_by(|file| file.path.as_str().as_bytes().cmp(path.as_bytes()))
        .ok()
        .and_then(|index| files.get(index))
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
    let steps = step(core, before, after)?;
    let after_files = core.files_of(after)?;
    let mut edited = Vec::new();
    for step in &steps {
        for op in &step.ops {
            let then = find(&after_files, op.path().as_str()).map(|file| file.blob);
            let now = current(core, folder, settings, op.path())?;
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
    folder: &Path,
    settings: &Settings,
    path: &MatterPath,
) -> Result<Current, Error> {
    let abs = folder.join(path.as_str());
    let metadata = match std::fs::symlink_metadata(&abs) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Current::Missing),
        Err(err) => return Err(Error::io("stat", abs, err)),
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

/// Whether the entry at `abs` is one the crate never reads or writes: not a regular file,
/// cloud-only, or over the cap.
fn skipped(settings: &Settings, abs: &Path, metadata: &Metadata) -> bool {
    let cap = settings.limits.file_cap_bytes.min(u64::from(u32::MAX));
    !metadata.is_file() || settings.probe.is_cloud_only(abs, metadata) || metadata.len() > cap
}

/// Whether undo may write or delete `path`: no folder on the way is a symlink (or a junction,
/// or not a folder), the entry itself is a regular file or absent, and it is not cloud-only or
/// over the cap. On Windows, no part of the path is a device name.
fn writable(folder: &Path, settings: &Settings, path: &MatterPath) -> Result<bool, Error> {
    let mut at = folder.to_owned();
    let mut parts = path.as_str().split('/').peekable();
    while let Some(part) = parts.next() {
        #[cfg(windows)]
        if clauseo_protocol::is_windows_reserved_name(part) {
            return Ok(false);
        }
        at.push(part);
        let metadata = match std::fs::symlink_metadata(&at) {
            Ok(metadata) => metadata,
            // Nothing there: the rest of the path is made fresh.
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(err) => return Err(Error::io("stat", &at, err)),
        };
        if parts.peek().is_some() {
            if !metadata.is_dir() {
                return Ok(false);
            }
        } else {
            return Ok(!skipped(settings, &at, &metadata));
        }
    }
    Ok(true)
}

/// See [`crate::Matter::undo`].
pub(crate) fn undo(
    core: &mut Core,
    folder: &Path,
    settings: &Settings,
    request: &UndoRequest<'_>,
) -> Result<Undone, Error> {
    let steps = step(core, request.before, request.after)?;
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
    for step in &acting {
        for op in &step.ops {
            apply(core, folder, op)?;
        }
    }
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

/// Carries out one operation: a write by temp file and rename (making missing folders), or a
/// delete (an already missing file is fine).
fn apply(core: &Core, folder: &Path, op: &Op) -> Result<(), Error> {
    let abs = folder.join(op.path().as_str());
    match op {
        Op::Write(_, blob) => {
            if let Some(parent) = abs.parent() {
                write::create_dir_all(parent).map_err(|err| Error::io("create", parent, err))?;
            }
            write::replace(&abs, &mut |file| {
                core.history.read_blob(*blob, file).map(drop)
            })
        }
        Op::Delete(_) => match write::remove_file(&abs) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(Error::io("delete", abs, err)),
        },
    }
}
