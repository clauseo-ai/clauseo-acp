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

/// The owner files Office makes beside a file it has open: `~$` + the name, and `~$` + the name
/// without its first two characters when it has more than two.
pub(crate) fn owner_names(name: &str) -> Vec<String> {
    let mut names = vec![format!("~${name}")];
    if name.chars().count() > 2 {
        let rest: String = name.chars().skip(2).collect();
        names.push(format!("~${rest}"));
    }
    names
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
