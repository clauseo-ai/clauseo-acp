//! Baselines in a diff of two checkpoints: a file a checkpoint between them read for the first
//! time is compared from the content it had there, so one unchanged since is no change, in
//! either direction and across any number of checkpoints, as each checkpoint's own diff had it.

use std::collections::BTreeMap;

use clauseo_protocol::MatterPath;

use super::VersionEntry;
use crate::Error;
use crate::history::{HistoryStore, Oid};

/// Sets, for each baseline of `baselines` (path, and the tree of the checkpoint that read it),
/// its content there on the `earlier` side and its content in `later_tree` on the `later` side
/// (both sorted by path bytes, as `changed_files` lists them), each left out when there is
/// none.
pub(super) fn pin(
    history: &impl HistoryStore,
    baselines: &BTreeMap<MatterPath, Oid>,
    later_tree: Oid,
    earlier: &mut Vec<VersionEntry>,
    later: &mut Vec<VersionEntry>,
) -> Result<(), Error> {
    for (path, tree) in baselines {
        let then = history.file_at(*tree, path.as_str())?;
        let now = history.file_at(later_tree, path.as_str())?;
        set(earlier, path, then);
        set(later, path, now);
    }
    Ok(())
}

/// `files` (sorted by path bytes) with `path` holding `blob`, or without it for `None`.
pub(crate) fn set(files: &mut Vec<VersionEntry>, path: &MatterPath, blob: Option<Oid>) {
    let at =
        files.binary_search_by(|file| file.path.as_str().as_bytes().cmp(path.as_str().as_bytes()));
    match (at, blob) {
        (Ok(index), Some(blob)) => {
            if let Some(file) = files.get_mut(index) {
                file.blob = blob;
            }
        }
        (Ok(index), None) => {
            files.remove(index);
        }
        (Err(index), Some(blob)) => files.insert(
            index,
            VersionEntry {
                path: path.clone(),
                blob,
            },
        ),
        (Err(_), None) => {}
    }
}
