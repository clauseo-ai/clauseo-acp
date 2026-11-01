//! Which files are open in another program, so an undo or a restore can refuse before writing
//! anything, and the host can refuse an agent's permission request to write one ([`held_open`]).

use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterPath;

use crate::refuse::spellings;

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

/// [`held_open`] for a path as an agent names it: absolute, or relative to `folder` (the
/// session's working folder). The path counts only when it lies in `folder` ([`matter_path_in`]);
/// then the matter path it names and what shows it held open, if anything does.
pub fn held_open_at(folder: &Path, path: &Path) -> Option<(MatterPath, HeldOpen)> {
    let inside = matter_path_in(folder, path)?;
    let root = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    held_open(&root, &inside).map(|held| (inside, held))
}

/// The matter path `path` (absolute, or relative to `folder`) names in `folder`, or `None`
/// when it lies outside. Both sides are resolved with `dunce::canonicalize` (symlinks, `..`,
/// Windows' `\\?\` prefix), a missing file through its parent, and compared by components as
/// refused folders are: ignoring case on macOS and Windows, exactly elsewhere. The rest of the
/// path keeps the spelling the file system gave it. When either side does not resolve, the path's
/// text is compared with the folder's instead: no `.` or `..` part, and the folder's text as a
/// prefix.
pub fn matter_path_in(folder: &Path, path: &Path) -> Option<MatterPath> {
    let joined = folder.join(path);
    if let (Ok(root), Some(target)) = (dunce::canonicalize(folder), resolved(&joined)) {
        return relative_to(&root, &target);
    }
    let relative = if path.is_absolute() {
        path.strip_prefix(folder).ok()?
    } else {
        path
    };
    MatterPath::from_relative(relative)
}

/// `path` canonicalised, or for a missing file its canonical parent joined with its name.
fn resolved(path: &Path) -> Option<PathBuf> {
    if let Ok(found) = dunce::canonicalize(path) {
        return Some(found);
    }
    let name = match path.components().next_back()? {
        Component::Normal(name) => name,
        Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir => {
            return None;
        }
    };
    let parent = dunce::canonicalize(path.parent()?).ok()?;
    Some(parent.join(name))
}

/// The matter path of the canonical `target` under the canonical `root`, compared by
/// [`spellings`].
fn relative_to(root: &Path, target: &Path) -> Option<MatterPath> {
    let parts: Vec<Component<'_>> = target.components().collect();
    for outer in spellings(root) {
        for key in spellings(target) {
            if key.len() > outer.len() && key.starts_with(&outer) {
                let rest = parts.get(parts.len().checked_sub(key.len() - outer.len())?..)?;
                return MatterPath::from_relative(&rest.iter().collect::<PathBuf>());
            }
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
