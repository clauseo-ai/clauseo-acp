//! Undoing a step: put back, file by file, what the step between two checkpoints changed.
//! Everything moves forward: the result is a new `undo` checkpoint, so an undo can be undone.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::{ChangeKind, CheckpointId, CheckpointKind, FileChoice, MatterPath};

use crate::checkpoint::{Core, SizedFile, Spec};
use crate::history::{HistoryStore, Oid};
use crate::stat::{EntryState, Stat, find_by_path};
use crate::types::{CheckpointOutcome, Settings, TimelineEntry, UndoRequest, Undone};
use crate::walk::{self, FoundKind};
use crate::{Error, fsync, locks, write};

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

/// Folder listings, each read once, to tell whether a path names its entries by their exact
/// bytes. On a case- or normalisation-insensitive file system (APFS by default, NTFS)
/// `symlink_metadata` of `memo.txt` also finds `Memo.txt`, which the history counts as
/// another file.
#[derive(Default)]
struct Names {
    listed: HashMap<PathBuf, HashSet<OsString>>,
}

impl Names {
    /// The metadata of `folder`/`path`, taken without following a symlink, when every part of
    /// `path` is an entry of exactly that name; `None` when something is missing or only an
    /// entry spelled otherwise is there.
    fn metadata(&mut self, folder: &Path, path: &MatterPath) -> Result<Option<Metadata>, Error> {
        let abs = folder.join(path.as_str());
        let metadata = match std::fs::symlink_metadata(&abs) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(Error::io("stat", abs, err)),
        };
        let mut dir = folder.to_owned();
        for part in path.as_str().split('/') {
            if !self.holds(&dir, part)? {
                return Ok(None);
            }
            dir.push(part);
        }
        Ok(Some(metadata))
    }

    /// Whether the folder `dir` holds an entry named exactly `name`.
    fn holds(&mut self, dir: &Path, name: &str) -> Result<bool, Error> {
        if !self.listed.contains_key(dir) {
            let mut names = HashSet::new();
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => Some(entries),
                Err(err) if err.kind() == io::ErrorKind::NotFound => None,
                Err(err) => return Err(Error::io("read folder", dir, err)),
            };
            for entry in entries.into_iter().flatten() {
                let entry = entry.map_err(|err| Error::io("read folder", dir, err))?;
                names.insert(entry.file_name());
            }
            self.listed.insert(dir.to_owned(), names);
        }
        Ok(self
            .listed
            .get(dir)
            .is_some_and(|names| names.contains(OsStr::new(name))))
    }
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

/// Whether the entry at `abs` is one the crate never reads or writes, by the walk's rule:
/// not a regular file, cloud-only, or over the cap.
fn skipped(settings: &Settings, abs: &Path, metadata: &Metadata) -> bool {
    let cap = settings.limits.effective_cap();
    walk::classify(settings.probe.as_ref(), abs, metadata, cap) != Some(FoundKind::File)
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
    apply(core, folder, &acting)?;
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

/// Carries out the steps' operations so that a failure while writing changes nothing, and a
/// deletion never removes a file just restored:
///
/// 1. every write goes to a synced temp file beside its target (making missing folders);
/// 2. every target is stat'd again against the cache the `outside-edits` checkpoint just
///    wrote: one that changed since fails the undo with `matter.file_locked`;
/// 3. the deletions are made;
/// 4. the temp files are renamed over their targets, each folder synced.
///
/// Deleting before renaming matters on a case- or normalisation-insensitive file system: undoing
/// a rename of `Brief.txt` to `brief.txt` deletes `brief.txt`, which is the same file as
/// `Brief.txt` there; deleting after the rename would remove what was just put back.
fn apply(core: &Core, folder: &Path, steps: &[&Step]) -> Result<(), Error> {
    let mut staged = Vec::new();
    for op in steps.iter().flat_map(|step| &step.ops) {
        if let Op::Write(path, blob) = op {
            let abs = make_parents(folder, path)?;
            let file = write::stage(&abs, &mut |file| {
                core.history.read_blob(*blob, file).map(drop)
            })?;
            staged.push(file);
        }
    }
    let moved = moved_since(core, folder, steps)?;
    if !moved.is_empty() {
        return Err(Error::FileLocked { paths: moved });
    }
    for op in steps.iter().flat_map(|step| &step.ops) {
        if let Op::Delete(path) = op {
            let abs = folder.join(path.as_str());
            match write::remove_file(&abs) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => return Err(Error::io("delete", abs, err)),
            }
            let dir = abs.parent().unwrap_or(folder);
            fsync::sync_dir(dir).map_err(|err| Error::io("sync", dir, err))?;
        }
    }
    for file in staged {
        file.commit()?;
    }
    Ok(())
}

/// The targets of `steps` whose stat differs from the cache's entry (or that appeared or went
/// since), sorted: something wrote to them after the cache was written.
fn moved_since(core: &Core, folder: &Path, steps: &[&Step]) -> Result<Vec<MatterPath>, Error> {
    let mut names = Names::default();
    let mut moved = Vec::new();
    for op in steps.iter().flat_map(|step| &step.ops) {
        let path = op.path();
        let now = names
            .metadata(folder, path)?
            .map(|metadata| Stat::from_metadata(&metadata));
        let then = core.cache.get(path.as_str()).map(|entry| entry.stat);
        if now != then {
            moved.push(path.clone());
        }
    }
    moved.sort();
    moved.dedup();
    Ok(moved)
}

/// `folder`/`path`, after making each missing folder on the way (and syncing the folder
/// that holds it). [`writable`] already refused a path through a symlink.
fn make_parents(folder: &Path, path: &MatterPath) -> Result<std::path::PathBuf, Error> {
    let mut at = folder.to_owned();
    let mut parts = path.as_str().split('/').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            at.push(part);
            break;
        }
        let parent = at.clone();
        at.push(part);
        match write::create_dir(&at) {
            Ok(()) => fsync::sync_dir(&parent).map_err(|err| Error::io("sync", &parent, err))?,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(Error::io("create", &at, err)),
        }
    }
    Ok(at)
}
