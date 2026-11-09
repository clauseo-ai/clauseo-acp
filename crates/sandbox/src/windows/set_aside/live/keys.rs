//! The live check's keys: a watched folder's paths, one name per part, and what they name.

use std::ffi::OsString;
use std::path::{Component, Path};

use super::super::files::FileId;
use super::super::names::Key;
use super::super::repo_names::{is_git, is_head};
use super::super::scan::same_key;

/// Whether `key` is below a `.git`.
pub(super) fn below_git(key: &[OsString]) -> bool {
    let parents = key.get(..key.len().saturating_sub(1)).unwrap_or_default();
    parents.iter().any(|part| is_git(part))
}

/// Whether `list` holds `key` (as [`same_key`] compares), with the file ID `id` when one is
/// given.
pub(super) fn holds(list: &[(Key, FileId)], key: &[OsString], id: Option<FileId>) -> bool {
    list.iter()
        .any(|(known, at)| id.is_none_or(|id| *at == id) && same_key(known, key))
}

/// Whether `key`'s own name opens as `.git`.
pub(super) fn ends_in_git(key: &[OsString]) -> bool {
    key.last().is_some_and(|name| is_git(name))
}

/// Whether `key` names a `.git`, or a `HEAD` outside every `.git`: what the watch knows.
pub(super) fn repo_part(key: &[OsString]) -> bool {
    ends_in_git(key) || key.last().is_some_and(|name| is_head(name))
}

/// `path`'s parts, when each is a plain name (no root, drive, `.` or `..`).
pub(super) fn key_of(path: &Path) -> Option<Key> {
    let mut key = Vec::new();
    for part in path.components() {
        match part {
            Component::Normal(name) => key.push(name.to_owned()),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => {
                return None;
            }
        }
    }
    (!key.is_empty()).then_some(key)
}
