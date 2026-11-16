//! What a reader of the version history asks of the folder as it is now, one path at a time:
//! [`holds_now`], whether the folder holds a file by the checkpoint walk's rules. With the
//! `test-support` feature, [`checkpoint_with_old_files`] writes a checkpoint as a build from
//! before the dot-folder rule would have, for tests of what an old tree can hold.

use std::io;

use clauseo_protocol::MatterPath;

use super::Matter;
use crate::no_download::NoDownload;
use crate::writeback::Names;
use crate::{Error, rules, walk};

/// Whether the folder holds the file `path` now, as the checkpoint walk would find it: not an
/// excluded path ([`crate::is_excluded_path`]), every folder on the way a real folder (a
/// symlink is never followed), the file itself a regular file (cloud-only and over the size cap
/// count: the walk lists them too), and every part of `path` an entry of exactly that name, byte
/// for byte (on APFS or NTFS `memo.txt` does not find `Memo.txt`, which the history counts as
/// another file). Only stats and folder listings, under the no-download guard: no file is
/// opened. Missing anywhere on the way is `Ok(false)`; any other failed stat or listing is
/// [`Error::Io`]. Needs no writer and reads no history.
pub fn holds_now(matter: &Matter, path: &MatterPath) -> Result<bool, Error> {
    if rules::is_excluded_path(path.as_str()) {
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
