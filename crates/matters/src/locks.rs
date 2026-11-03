//! Which files are open in another program, so an undo or a restore can refuse before writing
//! anything, and the host can refuse an agent's permission request to write one ([`held_open`]).

use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterPath;
use unicode_normalization::{UnicodeNormalization, is_nfc};

use crate::refuse::{Fold, keys_by};

/// What shows that a file is open in another program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldOpen {
    /// An Office owner file sits beside it; the name of the first one found (`~$ntract.docx`).
    OwnerFile(String),
    /// Windows only: opening it with no sharing failed with a sharing or lock violation.
    SharingViolation,
}

/// Whether `path` (relative to `folder`) looks open in another program, such as Word, and
/// what shows it: an Office owner file beside it (see [`owner_names`]), checked first, by its
/// exact name and then, for a name that is not all ASCII, in any Unicode form
/// ([`owner_in_another_form`]); on Windows also a sharing or lock violation when it exists and
/// is opened with no sharing. A missing file with no owner file, and any other error, is `None`.
/// One `symlink_metadata` per owner name (at most three), a `read_dir` of the parent for a
/// non-ASCII name none of them finds (at most [`MAX_LISTED`] entries; an owner file past them,
/// or a listing that fails, is not found: it fails open, as any other error does), and on
/// Windows one open. The rule undo and restore refuse by.
pub fn held_open(folder: &Path, path: &MatterPath) -> Option<HeldOpen> {
    held_open_by(folder, path, None)
}

/// [`held_open`] for a caller that has listed the parent folder already: `owner_files` are its
/// entries starting `~$`, and the lookup by another Unicode form compares against them instead
/// of listing the folder again. The host's guard lists each folder once and asks for every file
/// in it, so a folder of many non-ASCII names costs one listing, not one per file.
pub fn held_open_beside(
    folder: &Path,
    path: &MatterPath,
    owner_files: &[String],
) -> Option<HeldOpen> {
    held_open_by(folder, path, Some(owner_files))
}

/// The most entries the lookup by another Unicode form reads from one folder.
const MAX_LISTED: usize = 20_000;

/// [`held_open`], with the parent's owner files given (`Some`) or listed here (`None`).
fn held_open_by(folder: &Path, path: &MatterPath, given: Option<&[String]>) -> Option<HeldOpen> {
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
    if let Some(owner) = owner_in_another_form(parent, name, given) {
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
/// session's working folder). The path counts only when it lies in `folder`, resolved as
/// [`matter_path_in`] resolves it but with the folder's parts compared composed (NFC) everywhere
/// ([`Fold::Safety`]): a match only refuses, so a folder spelled in another Unicode form counts;
/// then the matter path it names and what shows it held open, if anything does.
pub fn held_open_at(folder: &Path, path: &Path) -> Option<(MatterPath, HeldOpen)> {
    let inside = path_in(folder, path, Fold::Safety)?;
    let root = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    held_open(&root, &inside).map(|held| (inside, held))
}

/// The matter path `path` (absolute, or relative to `folder`) names in `folder`, or `None`
/// when it lies outside. Both sides are resolved with `dunce::canonicalize` (symlinks, `..`,
/// Windows' `\\?\` prefix), a missing file through its parent, and compared by components as
/// this OS's file system compares names ([`Fold::FileSystem`]: ignoring case on macOS and
/// Windows, and Unicode form on macOS; byte for byte elsewhere, since ext4 and NTFS tell the
/// forms apart and the answer lets a path count as the matter's). The rest of the path keeps the
/// spelling the file system gave it. When either side does not resolve, the path's
/// text is compared with the folder's instead: no `.` or `..` part, and the folder's text as a
/// prefix. A `..` after a missing folder follows the OS, as a write through the path would:
/// Windows drops the pair by its text (`sub/../x` is `x` even with no `sub`), so the path
/// resolves; Unix needs `sub` to exist, so it does not, and the write would fail with ENOENT.
pub fn matter_path_in(folder: &Path, path: &Path) -> Option<MatterPath> {
    path_in(folder, path, Fold::FileSystem)
}

/// [`matter_path_in`] with the folder's parts compared by `fold`.
fn path_in(folder: &Path, path: &Path, fold: Fold) -> Option<MatterPath> {
    let joined = folder.join(path);
    if let (Ok(root), Some(target)) = (dunce::canonicalize(folder), resolved(&joined)) {
        return relative_to(&root, &target, fold);
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
/// [`keys_by`] with `fold`.
fn relative_to(root: &Path, target: &Path, fold: Fold) -> Option<MatterPath> {
    let parts: Vec<Component<'_>> = target.components().collect();
    for outer in keys_by(root, fold) {
        for key in keys_by(target, fold) {
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

/// The owner file in `parent` that names `name` in another Unicode form, for a name that is not
/// all ASCII: an entry starting `~$` whose composed (NFC) name, folded as [`fold_name`] does,
/// is one of [`owner_names`] of the document's name as written, composed (NFC) or decomposed
/// (NFD), each composed. Ext4 and NTFS tell the forms apart, so the exact stats miss an owner
/// file that Word or a sync made in the other form (`क़` as U+0958 or as `क` + U+093C), and a
/// write would race the program holding it. Compares against `given` (the parent's `~$`
/// entries, listed by the caller) or else one `read_dir` of `parent` of at most [`MAX_LISTED`]
/// entries, only when the exact stats found nothing and the name holds a non-ASCII character;
/// its first match by name. A failed listing finds nothing (fails open).
fn owner_in_another_form(parent: &Path, name: &str, given: Option<&[String]>) -> Option<String> {
    if name.is_ascii() {
        return None;
    }
    let owners: Vec<(String, String)> = match given {
        Some(owners) => owners
            .iter()
            .filter(|entry| entry.starts_with("~$"))
            .map(|entry| (fold_name(entry), entry.clone()))
            .collect(),
        None => std::fs::read_dir(parent)
            .ok()?
            .take(MAX_LISTED)
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|entry| entry.starts_with("~$"))
            .map(|entry| (fold_name(&entry), entry))
            .collect(),
    };
    if owners.is_empty() {
        return None;
    }
    // The name as written, composed and decomposed, each once: most names are NFC already, and
    // a Devanagari name with no nukta letter is NFD too. Owner names cut from the NFC form are
    // NFC already (a suffix of an NFC string is NFC), so off macOS and Windows they need no fold.
    let composed: String = if is_nfc(name) {
        name.to_owned()
    } else {
        name.nfc().collect()
    };
    let decomposed: String = name.nfd().collect();
    let mut wanted: Vec<String> = owner_names(&composed)
        .into_iter()
        .map(|owner| {
            if cfg!(any(target_os = "macos", windows)) {
                fold_name(&owner)
            } else {
                owner
            }
        })
        .collect();
    for form in [name, decomposed.as_str()] {
        if form != composed {
            wanted.extend(owner_names(form).iter().map(|owner| fold_name(owner)));
        }
    }
    let mut found: Vec<&String> = owners
        .iter()
        .filter(|(folded, _)| wanted.contains(folded))
        .map(|(_, entry)| entry)
        .collect();
    found.sort_unstable();
    found.first().map(|entry| (*entry).clone())
}

/// A name as the safety checks compare it: composed (NFC) everywhere, and lower case on the
/// case-insensitive file systems (macOS, Windows).
fn fold_name(name: &str) -> String {
    if name.is_ascii() {
        case(name)
    } else {
        case(name).nfc().collect()
    }
}

/// A name lower case on the case-insensitive file systems (macOS, Windows); as it is elsewhere.
fn case(name: &str) -> String {
    if cfg!(any(target_os = "macos", windows)) {
        name.to_lowercase()
    } else {
        name.to_owned()
    }
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
