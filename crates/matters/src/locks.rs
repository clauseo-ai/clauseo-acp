//! Which files are open in another program, so an undo can refuse before writing anything.

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterPath;

/// The files among `paths` (relative to `folder`) that look open in another program, sorted by
/// path bytes and deduplicated. A file is locked if an Office owner file sits beside it (see
/// [`owner_names`]); on Windows also if it exists and opening it with no sharing fails with a
/// sharing or lock violation. A missing file, and any other error, is not locked.
pub(crate) fn locked(folder: &Path, paths: &[MatterPath]) -> Vec<MatterPath> {
    let mut locked: Vec<MatterPath> = paths
        .iter()
        .filter(|path| is_locked(folder, path))
        .cloned()
        .collect();
    locked.sort_unstable_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    locked.dedup();
    locked
}

/// The owner files Office makes beside a file it has open: `~$` + the name, `~$` + the name
/// without its first character, and `~$` + the name without its first two, each when the name
/// is longer than what it drops. Word keeps a stem of up to 6 characters whole, drops one at 7
/// and two from 8 (research 38, `word-lock.json` `name_lengths`); checking all three costs one
/// stat each and needs no stem rule.
pub(crate) fn owner_names(name: &str) -> Vec<String> {
    let count = name.chars().count();
    (0..3)
        .filter(|&dropped| dropped == 0 || count > dropped)
        .map(|dropped| format!("~${}", name.chars().skip(dropped).collect::<String>()))
        .collect()
}

fn is_locked(folder: &Path, path: &MatterPath) -> bool {
    let absolute: PathBuf = path
        .as_str()
        .split('/')
        .fold(folder.to_path_buf(), |acc, part| acc.join(part));
    let (Some(parent), Some(name)) = (absolute.parent(), path.as_str().rsplit('/').next()) else {
        return false;
    };
    if owner_names(name)
        .iter()
        .any(|owner| parent.join(owner).symlink_metadata().is_ok())
    {
        return true;
    }
    #[cfg(windows)]
    {
        /// `ERROR_SHARING_VIOLATION` and `ERROR_LOCK_VIOLATION`.
        const SHARING_VIOLATION: i32 = 32;
        const LOCK_VIOLATION: i32 = 33;
        if let Err(err) = crate::write::open_exclusive(&absolute) {
            return matches!(err.raw_os_error(), Some(SHARING_VIOLATION | LOCK_VIOLATION));
        }
    }
    false
}

#[cfg(test)]
mod tests;
