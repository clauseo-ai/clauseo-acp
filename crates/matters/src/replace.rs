//! Replacing one file of the folder with content made outside it: a Word file whose tracked
//! changes the host accepted or rejected with stetdoc on a private copy. Like a restore it moves
//! forward: an `outside-edits` checkpoint first when the folder changed, then the write, then a
//! `revisions` checkpoint, so `Matter::restore` of the checkpoint before puts the file back.
//!
//! The order, and what a crash between two steps leaves (nothing is committed or written before
//! step 4):
//!
//! 1. The ids are checked.
//! 2. One walk of the folder against the stat cache, as any checkpoint's.
//! 3. The checks: the file is not held open (`matter.file_locked`), is one the walk read (not
//!    over the size cap, not cloud-only: `word.edit_failed`, as a retry would meet the same),
//!    may be written, and its content as walked is exactly the bytes the new content was made
//!    from (`word.revisions_stale` otherwise).
//! 4. The walk is committed as the `outside-edits` checkpoint if the folder changed.
//! 5. The new content is stored as a blob, then `writeback::apply` stages it beside the file,
//!    checks the file's stat against that checkpoint's cache again and renames. A file changed
//!    since the walk is `word.revisions_stale`, one held open by then `matter.file_locked`;
//!    either way nothing is written. A crash here leaves the file old or new, never torn, and a
//!    new one recorded by the next checkpoint as outside edits.
//! 6. The `revisions` checkpoint.
//!
//! Once the rename is made the replace succeeds, whatever fails after it: a folder sync in
//! step 5, or the walk of step 6 (a file another program holds anywhere in the folder fails
//! it). The answer then has `before` and no `revisions` checkpoint; the next checkpoint records
//! the new content.

use std::path::Path;

use clauseo_protocol::{CheckpointId, CheckpointKind, MatterPath};

use crate::checkpoint::{Core, OnHeld, Spec};
use crate::history::{HistoryStore, Oid};
use crate::no_download::NoDownload;
use crate::stat::{EntryState, find_by_path};
use crate::types::{Checkpoint, CheckpointOutcome, ReplaceFile, Replaced, Settings, TimelineEntry};
use crate::writeback::{self, Failed, Op, writable};
use crate::{Error, locks};

/// See [`crate::Matter::replace_file`]. The steps are the module's.
pub(crate) fn replace(
    core: &mut Core,
    folder: &Path,
    settings: &Settings,
    request: &ReplaceFile<'_>,
) -> Result<Replaced, Error> {
    let path = request.path;
    // 1. The ids.
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
    // 2. One walk.
    let walked = core
        .scan(folder, settings, None, &mut |_| {}, OnHeld::Report)?
        .ok_or_else(|| Error::history("walk", "cancelled without a cancel flag"))?;
    // 3. The checks.
    let held = walked.held().contains(path);
    if held || !locks::locked(folder, std::slice::from_ref(path)).is_empty() {
        return Err(Error::FileLocked {
            paths: vec![path.clone()],
        });
    }
    let found = find_by_path(walked.entries(), path.as_str(), |entry| entry.path.as_str())
        .map(|entry| entry.state);
    if matches!(found, Some(EntryState::Skipped | EntryState::Unread(_))) {
        return Err(Error::NotVersioned { path: path.clone() });
    }
    let expected = hash_of(core, settings, request.expected)?;
    if found != Some(EntryState::Read(expected)) || !writable(folder, settings, path)? {
        return Err(changed(path));
    }
    // 4. The outside-edits checkpoint.
    let outside = core.commit(
        walked,
        &spec(request.outside_edits, CheckpointKind::OutsideEdits, request),
    )?;
    let before_commit = outside.commit.clone();
    let (outside_edits, before) = match outside.outcome {
        CheckpointOutcome::Created(id) => (Some(outside), id),
        CheckpointOutcome::Reused(id) => (None, id),
    };
    // 5. Store, stage, check again, rename.
    let blob = store(core, settings, request.new)?;
    let op = Op::Write(path.clone(), blob);
    let checkpoint = match writeback::apply_or_partial(core, folder, &[&op]) {
        // 6. The revisions checkpoint.
        Ok(_deleted) => after_write(take_revisions(core, folder, settings, request)),
        // Written, then a sync failed: no checkpoint over a rename that may not last.
        Err(Failed {
            error,
            changed: true,
        }) => after_write(Err(error)),
        // The stat moved since the walk: changed, unless a program holds it now.
        Err(Failed {
            error: Error::FileLocked { .. },
            ..
        }) if locks::held_open(folder, path).is_none() => {
            return Err(changed(path));
        }
        Err(Failed { error, .. }) => return Err(error),
    };
    Ok(Replaced {
        outside_edits,
        before,
        before_commit,
        checkpoint,
    })
}

/// See [`crate::Matter::read_local`].
pub(crate) fn read_local(
    folder: &Path,
    settings: &Settings,
    path: &MatterPath,
) -> Result<Vec<u8>, Error> {
    let file = folder.join(path.as_str());
    let not_versioned = || Error::NotVersioned { path: path.clone() };
    let guard = NoDownload::begin(settings.probe.as_ref());
    let metadata = std::fs::metadata(&file).map_err(|err| {
        let held = locks::held_at_read(&err);
        read_failed(&file, path, "stat", err, held)
    })?;
    if guard.is_cloud_only(&file, &metadata) {
        return Err(not_versioned());
    }
    if metadata.len() > settings.limits.effective_cap() {
        return Err(not_versioned());
    }
    match std::fs::read(&file) {
        Ok(bytes) => Ok(bytes),
        Err(err) if crate::no_download::not_downloaded(&err) => Err(not_versioned()),
        Err(err) => {
            let held = locks::held_at_read(&err);
            Err(read_failed(&file, path, "read", err, held))
        }
    }
}

/// A failed `what` ("stat", "read") of `path`, at `file`, in [`read_local`]: `matter.file_locked`
/// naming it when `held` (another program holds it with no sharing, [`locks::held_at_read`]: a
/// Windows sharing or lock violation), else the I/O error.
fn read_failed(
    file: &Path,
    path: &MatterPath,
    what: &'static str,
    err: std::io::Error,
    held: bool,
) -> Error {
    if held {
        return Error::FileLocked {
            paths: vec![path.clone()],
        };
    }
    Error::io(what, file, err)
}

/// The `revisions` checkpoint, or none when what came after the rename failed: the file is
/// written, so the replace succeeds and the next checkpoint records it.
fn after_write(taken: Result<Checkpoint, Error>) -> Option<Checkpoint> {
    taken
        .inspect_err(|error| {
            // The code, and an I/O failure's op and kind, never its path (ADR 0047).
            let (op, kind) = if let Error::Io { op, source, .. } = error {
                (Some(*op), Some(source.kind()))
            } else {
                (None, None)
            };
            tracing::warn!(
                code = %error.code(),
                op,
                kind = ?kind,
                "a replaced file was written but not checkpointed"
            );
        })
        .ok()
}

/// Takes the `revisions` checkpoint of `request`, after its file is written. With the
/// `test-support` feature, first the failure `test_support::fail_after_replace_write` armed for
/// the file.
fn take_revisions(
    core: &mut Core,
    folder: &Path,
    settings: &Settings,
    request: &ReplaceFile<'_>,
) -> Result<Checkpoint, Error> {
    #[cfg(feature = "test-support")]
    crate::test_support::after_replace_write(&folder.join(request.path.as_str()))?;
    core.take(
        folder,
        settings,
        &spec(request.checkpoint, CheckpointKind::Revisions, request),
    )
}

/// [`Error::ChangedSince`] for `path`.
fn changed(path: &MatterPath) -> Error {
    Error::ChangedSince { path: path.clone() }
}

/// The blob `file` (outside the folder) would be, stored nowhere.
fn hash_of(core: &mut Core, settings: &Settings, file: &Path) -> Result<Oid, Error> {
    let len = std::fs::metadata(file)
        .map_err(|err| Error::io("stat", file, err))?
        .len();
    let guard = NoDownload::begin(settings.probe.as_ref());
    Ok(core.history.hash_file(file, len, &guard)?.0)
}

/// `file` (outside the folder) stored as a blob, within the walk's size cap.
fn store(core: &mut Core, settings: &Settings, file: &Path) -> Result<Oid, Error> {
    let len = std::fs::metadata(file)
        .map_err(|err| Error::io("stat", file, err))?
        .len();
    let cap = settings.limits.effective_cap();
    let guard = NoDownload::begin(settings.probe.as_ref());
    Ok(core.history.store_file(file, len, cap, &guard)?.0)
}

/// The spec of a replace's `outside-edits` or `revisions` checkpoint: no thread, no turn.
fn spec<'a>(id: CheckpointId, kind: CheckpointKind, request: &ReplaceFile<'_>) -> Spec<'a> {
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
