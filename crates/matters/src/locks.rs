//! Which files are open in another program, so an undo or a restore can refuse before writing
//! anything, and the host can refuse an agent's permission request to write one ([`held_open`]).

use std::path::{Path, PathBuf};

use clauseo_protocol::MatterPath;

/// What shows that a file is open in another program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldOpen {
    /// An Office owner file sits beside it; the name of the first one found (`~$ntract.docx`).
    OwnerFile(String),
    /// Windows only: opening it with no sharing failed with a sharing or lock violation.
    SharingViolation,
}

/// Whether `path` (relative to `folder`) looks open in another program, such as Word, and
/// what shows it: an Office owner file beside it (see [`owner_names`]), checked first; on
/// Windows also a sharing or lock violation when it exists and is opened with no sharing. A
/// missing file with no owner file, and any other error, is `None`. One `symlink_metadata` per
/// owner name (at most three), and on Windows one open. The rule undo and restore refuse by.
pub fn held_open(folder: &Path, path: &MatterPath) -> Option<HeldOpen> {
    let absolute: PathBuf = path
        .as_str()
        .split('/')
        .fold(folder.to_path_buf(), |acc, part| acc.join(part));
    let (Some(parent), Some(name)) = (absolute.parent(), path.as_str().rsplit('/').next()) else {
        return None;
    };
    if let Some(owner) = owner_names(name)
        .into_iter()
        .find(|owner| parent.join(owner).symlink_metadata().is_ok())
    {
        return Some(HeldOpen::OwnerFile(owner));
    }
    #[cfg(windows)]
    {
        /// `ERROR_SHARING_VIOLATION` and `ERROR_LOCK_VIOLATION`.
        const SHARING_VIOLATION: i32 = 32;
        const LOCK_VIOLATION: i32 = 33;
        if let Err(err) = crate::write::open_exclusive(&absolute)
            && matches!(err.raw_os_error(), Some(SHARING_VIOLATION | LOCK_VIOLATION))
        {
            return Some(HeldOpen::SharingViolation);
        }
    }
    None
}

/// The files among `paths` (relative to `folder`) that look open in another program
/// ([`held_open`]), sorted by path bytes and deduplicated.
pub(crate) fn locked(folder: &Path, paths: &[MatterPath]) -> Vec<MatterPath> {
    let mut locked: Vec<MatterPath> = paths
        .iter()
        .filter(|path| held_open(folder, path).is_some())
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

#[cfg(test)]
mod tests;
