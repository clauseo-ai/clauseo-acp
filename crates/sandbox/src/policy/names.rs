//! Names and paths as macOS resolves them, for [`under`](super::under) and the policy's dedup
//! sets: one key per name, so two spellings APFS opens as one entry share it.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use caseless::Caseless;
use unicode_normalization::UnicodeNormalization;

/// The key of `name`. On macOS, Unicode canonical caseless matching: NFD of the full case fold
/// (CaseFolding.txt statuses C and F, not Turkic) of NFD, the folding a case-insensitive APFS
/// volume documents. So `क़` as U+0958 and as U+0915 U+093C, `É` and `é`, `ß` and `ss` share a
/// key. A name that is not UTF-8 folds ASCII case only. Elsewhere the name itself: NTFS and Linux
/// file systems keep NFC and NFD names as two files.
pub(super) fn name_key(name: &OsStr) -> Cow<'_, OsStr> {
    if !cfg!(target_os = "macos") {
        return Cow::Borrowed(name);
    }
    match name.to_str() {
        Some(text) if text.is_ascii() => {
            if text.bytes().any(|b| b.is_ascii_uppercase()) {
                Cow::Owned(OsString::from(text.to_ascii_lowercase()))
            } else {
                Cow::Borrowed(name)
            }
        }
        Some(text) => {
            let nfd: String = text.nfd().collect();
            let folded: String = nfd.chars().default_case_fold().collect();
            // The second NFD only when the fold changed something (most scripts have no case).
            let key = if folded == nfd {
                nfd
            } else {
                folded.nfd().collect()
            };
            Cow::Owned(OsString::from(key))
        }
        None => Cow::Owned(name.to_ascii_lowercase()),
    }
}

/// The key of `path`: each component's [`name_key`], joined. Two paths are [`same`](super::same)
/// exactly when their keys are equal, so a set of keys answers membership in one lookup.
pub(super) fn path_key(path: &Path) -> PathBuf {
    if !cfg!(target_os = "macos") {
        return path.to_owned();
    }
    path.components()
        .map(|c| name_key(c.as_os_str()))
        .fold(PathBuf::new(), |mut key, name| {
            key.push(name);
            key
        })
}

/// [`path_key`] of many paths that share folders, each folder's key made once.
#[derive(Default)]
pub(super) struct Keys {
    folders: HashMap<PathBuf, PathBuf>,
}

impl Keys {
    /// [`path_key`] of `path`.
    pub(super) fn of(&mut self, path: &Path) -> PathBuf {
        if !cfg!(target_os = "macos") {
            return path.to_owned();
        }
        let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
            return path_key(path);
        };
        let mut key = if let Some(key) = self.folders.get(folder) {
            key.clone()
        } else {
            let key = path_key(folder);
            self.folders.insert(folder.to_owned(), key.clone());
            key
        };
        key.push(name_key(name));
        key
    }
}

/// Whether `a` and `b` name the same entry: equal [`name_key`]s. On macOS two ASCII names
/// are compared without ASCII case alone, which is what their keys would do.
pub(super) fn same_name(a: &OsStr, b: &OsStr) -> bool {
    if cfg!(target_os = "macos") && a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    name_key(a) == name_key(b)
}
