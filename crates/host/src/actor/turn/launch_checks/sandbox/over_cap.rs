//! The matter's files over the checkpoint size cap (decision record 0038: never versioned, so
//! read-only in the jail, where Undo could not bring back what an agent wrote over them). Found
//! at each jailed launch by a walk of the matter on the seam's thread, by the cap the checkpoint
//! walk applies (`clauseo_matters::FILE_CAP_BYTES`, a file larger than it): a checkpoint lists
//! only the skipped files it saw new or changed, so its list is not the matter's.

use std::path::{Path, PathBuf};

/// The most entries the walk looks at before it stops: the files it found so far are
/// read-only, the rest keep the matter's write grant (as before R17), with one log line.
pub(crate) const MAX_ENTRIES: usize = 200_000;

/// Every regular file under `matter` larger than `cap` bytes, sorted. Links are not followed and
/// `.git` folders are not entered (the policy makes them read-only whole); a folder that does not
/// read is left out. File system calls: off the actor.
pub(crate) fn over_cap(matter: &Path, cap: u64) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![matter.to_path_buf()];
    let mut seen = 0_usize;
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                tracing::warn!(
                    found = found.len(),
                    "the matter is too large to look through for files over the size cap"
                );
                found.sort();
                return found;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if entry.file_name() != ".git" {
                    folders.push(entry.path());
                }
            } else if kind.is_file() && entry.metadata().is_ok_and(|meta| meta.len() > cap) {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found
}
