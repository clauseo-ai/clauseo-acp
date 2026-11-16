//! What a reader of the version history asks, one path at a time: [`holds_now`], whether the
//! folder holds a file now by the checkpoint walk's rules; [`version_file`], one version's file
//! by a single tree lookup, as [`Matter::files_at`] would list it; and [`in_version`], whether a
//! version holds a path at all. With the `test-support` feature, [`checkpoint_with_old_files`]
//! writes a checkpoint as a build from before the dot-folder rule would have, for tests of what an
//! old tree can hold.

use std::io;

use clauseo_protocol::{CheckpointId, MatterPath};

use super::Matter;
use crate::diff::content_id;
use crate::history::HistoryStore as _;
use crate::no_download::NoDownload;
use crate::types::VersionFile;
use crate::writeback::Names;
use crate::{Error, rules, walk};

/// Whether the folder holds the file `path` now, as the checkpoint walk would find it: not an
/// excluded path ([`crate::is_excluded_path`]), every folder on the way a real folder (a
/// symlink is never followed), the file itself a regular file (cloud-only and over the size cap
/// count: the walk lists them too), and every part of `path` an entry of exactly that name, byte
/// for byte (on APFS or NTFS `memo.txt` does not find `Memo.txt`, which the history counts as
/// another file). On Windows a part Windows would read as something else (a `:`, a device name
/// such as `CON`, a trailing dot or space: [`windows_invalid`]) is `Ok(false)` before any
/// stat. Only stats and folder listings, under the no-download guard: no file is opened.
/// Missing anywhere on the way is `Ok(false)`; any other failed stat or listing is
/// [`Error::Io`]. Needs no writer and reads no history.
pub fn holds_now(matter: &Matter, path: &MatterPath) -> Result<bool, Error> {
    if rules::is_excluded_path(path.as_str())
        || (cfg!(windows) && path.as_str().split('/').any(windows_invalid))
    {
        return Ok(false);
    }
    let guard = NoDownload::begin(matter.settings.probe.as_ref());
    let cap = matter.settings.limits.effective_cap();
    let mut at = matter.folder.clone();
    let mut parts = path.as_str().split('/').peekable();
    while let Some(part) = parts.next() {
        at.push(part);
        let metadata = match std::fs::symlink_metadata(&at) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(Error::io("stat", &at, err)),
        };
        let found = if parts.peek().is_some() {
            metadata.is_dir()
        } else {
            walk::classify(guard.probe(), &at, &metadata, cap).is_some()
        };
        if !found {
            return Ok(false);
        }
    }
    Ok(Names::default().metadata(&matter.folder, path)?.is_some())
}

/// Whether Windows would read the path part `part` as something other than that name: it holds
/// a `:` (an alternate data stream, or a drive), is a device name
/// ([`clauseo_protocol::is_windows_reserved_name`]), or ends in a dot or a space, which Windows
/// drops. [`MatterPath`] allows each of these, since macOS and Linux names may hold them.
pub(crate) fn windows_invalid(part: &str) -> bool {
    part.contains(':')
        || clauseo_protocol::is_windows_reserved_name(part)
        || part.ends_with('.')
        || part.ends_with(' ')
}

/// The file `path` of checkpoint `checkpoint`, as [`Matter::files_at`] would list it (its
/// content id and size), found by one lookup down the tree rather than by listing it; `None`
/// when the tree has no file there or `path` is excluded, as `files_at` leaves it out.
/// [`Error::UnknownCheckpoint`] when the timeline does not hold `checkpoint`. Needs only the
/// history.
pub fn version_file(
    matter: &Matter,
    checkpoint: CheckpointId,
    path: &MatterPath,
) -> Result<Option<VersionFile>, Error> {
    let core = &matter.core;
    let tree = core.timeline.tree(checkpoint)?;
    if rules::is_excluded_path(path.as_str()) {
        return Ok(None);
    }
    let Some(blob) = core.history.file_at(tree, path.as_str())? else {
        return Ok(None);
    };
    Ok(Some(VersionFile {
        path: path.clone(),
        content: content_id(blob)?,
        size: core.size_of(path.as_str(), blob)?,
    }))
}

/// Whether checkpoint `checkpoint`'s tree holds a file at `path`, excluded or not: one lookup
/// down the tree, reading neither the blob nor its size. [`Error::UnknownCheckpoint`] when the
/// timeline does not hold `checkpoint`.
pub fn in_version(
    matter: &Matter,
    checkpoint: CheckpointId,
    path: &MatterPath,
) -> Result<bool, Error> {
    let core = &matter.core;
    let tree = core.timeline.tree(checkpoint)?;
    Ok(core.history.file_at(tree, path.as_str())?.is_some())
}

/// Takes a `before-turn` checkpoint of `meta` as a build from before every dot-folder was left
/// out would have: the folder as the walk finds it, plus each of `old` (files in the folder
/// under `.claude`, `.codex` or another excluded folder) read and versioned as well. Tests only.
///
/// # Errors
///
/// What the walk, the store or the commit fails with.
#[cfg(feature = "test-support")]
pub fn checkpoint_with_old_files(
    matter: &mut Matter,
    meta: &crate::TurnMeta,
    old: &[&str],
) -> Result<crate::Checkpoint, Error> {
    crate::checkpoint::old_files::commit_with_old(
        &mut matter.core,
        &matter.folder,
        &matter.settings,
        meta,
        old,
    )
}

#[cfg(test)]
mod tests;
