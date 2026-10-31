//! Reading versions: the files that differ between two checkpoints (or a checkpoint and the
//! folder now), the files of a checkpoint, and a file's bytes at one. Nothing here writes: no
//! object, no cache, no checkpoint.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use clauseo_protocol::{CheckpointId, FileChange, MatterPath};

use crate::Error;
use crate::checkpoint::Core;
use crate::diff::{DiffFile, content_id, diff_by};
use crate::history::{HistoryStore, Oid};
use crate::stat::{CacheEntry, EntryState};
use crate::types::{CompareSide, Settings, VersionFile};

/// One file of a side of a comparison, with its size when it is known without a lookup.
struct SideFile {
    path: MatterPath,
    blob: Oid,
    size: Option<u64>,
}

impl DiffFile for SideFile {
    fn path(&self) -> &MatterPath {
        &self.path
    }

    fn blob(&self) -> Oid {
        self.blob
    }
}

/// See [`crate::Matter::compare`]. The folder is walked (once) only when a side names it.
pub(crate) fn compare(
    core: &Core,
    folder: &Path,
    settings: &Settings,
    from: CompareSide,
    to: CompareSide,
) -> Result<Vec<FileChange>, Error> {
    if let (CompareSide::Checkpoint(a), CompareSide::Checkpoint(b)) = (from, to) {
        return core.changes_between(a, b);
    }
    // At least one side is the folder now.
    let now = core.snapshot(folder, settings)?;
    // Files the folder holds but never versions (over the cap, or cloud-only and never read)
    // are left out of a checkpoint side too, so one now over the cap is not reported deleted
    // (or, the other way round, added), as a checkpoint reports it skipped.
    let unversioned: BTreeSet<&str> = now
        .iter()
        .filter(|entry| entry.state == EntryState::Skipped)
        .map(|entry| entry.path.as_str())
        .collect();
    let old = side(core, from, &now, &unversioned)?;
    let new = side(core, to, &now, &unversioned)?;
    diff_by(&old, &new, &BTreeSet::new(), &mut |file| match file.size {
        Some(size) => Ok(size),
        None => core.size_of(file.path.as_str(), file.blob),
    })
}

/// The files of one side, sorted by path.
fn side(
    core: &Core,
    side: CompareSide,
    now: &[CacheEntry],
    unversioned: &BTreeSet<&str>,
) -> Result<Vec<SideFile>, Error> {
    match side {
        CompareSide::Checkpoint(id) => Ok(core
            .tree_files(id)?
            .into_iter()
            .filter(|file| !unversioned.contains(file.path.as_str()))
            .map(|file| SideFile {
                path: file.path,
                blob: file.blob,
                size: None,
            })
            .collect()),
        CompareSide::Folder => {
            Ok(now
                .iter()
                .filter_map(|entry| {
                    let size = match entry.state {
                        EntryState::Read(_) => Some(u64::from(entry.stat.size)),
                        // A cloud-only file's stat is the placeholder's, not its content's.
                        EntryState::Unread(_) | EntryState::Skipped => None,
                    };
                    entry.state.blob().map(|blob| SideFile {
                        path: entry.path.clone(),
                        blob,
                        size,
                    })
                })
                .collect())
        }
    }
}

/// See [`crate::Matter::files_at`].
pub(crate) fn files_at(core: &Core, checkpoint: CheckpointId) -> Result<Vec<VersionFile>, Error> {
    core.files_of(checkpoint)?
        .into_iter()
        .map(|file| {
            Ok(VersionFile {
                content: content_id(file.blob)?,
                path: file.path,
                size: file.size,
            })
        })
        .collect()
}

/// See [`crate::Matter::read_file`].
pub(crate) fn read_file(
    core: &Core,
    checkpoint: CheckpointId,
    path: &MatterPath,
    out: &mut dyn Write,
) -> Result<u64, Error> {
    let tree = core.timeline.tree(checkpoint)?;
    let blob = core
        .history
        .file_at(tree, path.as_str())?
        .ok_or_else(|| Error::UnknownFile { path: path.clone() })?;
    core.history.read_blob(blob, path.as_str(), out)
}
