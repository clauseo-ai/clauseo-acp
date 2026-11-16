//! Step 3 of [`super::apply`]: every deletion made, each into the trash or deleted for good
//! ([`Removing`]), and its folder synced. A delete refused for the file's permissions (on
//! Windows only once the short retries are over, [`write::again_while_denied`]) passes the file
//! over: it is named in the caller's refused files, as it was, and the deletes and renames after
//! it go on.

use std::io;
use std::path::Path;

use clauseo_protocol::MatterPath;

use super::Op;
use crate::checkpoint::Core;
use crate::trash::Removing;
use crate::{Error, RefusedBecause, fsync, write};

/// Makes every deletion of `ops`, in order: the paths deleted (or found gone already), or the
/// error that stopped them and whether the folder changed by then. `any_change` is set by the
/// first delete made; a refused file goes in `refused`.
pub(super) fn delete_all(
    core: &Core,
    folder: &Path,
    ops: &[&Op],
    removing: &mut Removing<'_>,
    refused: &mut Vec<(MatterPath, RefusedBecause)>,
    any_change: &mut bool,
) -> Result<Vec<MatterPath>, (Error, bool)> {
    let mut deleted = Vec::new();
    for op in ops {
        let Op::Delete(path) = op else {
            continue;
        };
        // The path `moved_since` just checked, and nothing else.
        let abs = folder.join(path.as_str());
        let removed = write::again_while_denied(
            || removing.remove(core, &abs, path),
            |err: &io::Error| err.kind() == io::ErrorKind::PermissionDenied,
        );
        match removed {
            Ok(()) => {
                *any_change = true;
                deleted.push(path.clone());
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                deleted.push(path.clone());
                continue;
            }
            Err(err) => {
                let error = super::held_or(folder, path, Error::io("delete", abs, err));
                if let Some(because) = super::renames::refused(&error, None, false) {
                    refused.push((path.clone(), because));
                    continue;
                }
                return Err((error, *any_change));
            }
        }
        let dir = abs.parent().unwrap_or(folder);
        fsync::sync_dir(dir).map_err(|err| (Error::io("sync", dir, err), true))?;
    }
    Ok(deleted)
}
