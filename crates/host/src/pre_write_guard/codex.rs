//! The Codex side of the guard: a patch's targets, and the matter's `.codex` folder
//! (clauseo-acp-private#184).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{Reach, lexical};
use crate::network_paths::name_folds_to;

/// Whether `path`, taken from `base`, is `<folder>/.codex` or lies under it, resolved
/// by the network-safe walk (never when it is or leads to a network location, [`Reach::walked`],
/// its walked path used as it is) or lexically.
pub(super) fn in_codex_folder(folder: &Path, base: &Path, path: &Path, reach: &Reach) -> bool {
    let absolute = base.join(path);
    if let Some(walked) = reach.walked(&absolute)
        && let Some(inside) = clauseo_matters::matter_path_of_resolved(folder, &walked)
        && inside.as_str().split('/').next().is_some_and(is_codex)
    {
        return true;
    }
    lexical(&absolute)
        .strip_prefix(folder)
        .ok()
        .and_then(|relative| relative.components().next())
        .is_some_and(|first| first.as_os_str().to_str().is_some_and(is_codex))
}

/// Whether `name` is `.codex`, ignoring trailing dots and spaces and case, folded as a
/// case-insensitive volume folds names ([`name_folds_to`]: a refusal, so over-matching is safe).
fn is_codex(name: &str) -> bool {
    name_folds_to(OsStr::new(name.trim_end_matches(['.', ' '])), ".codex")
}

/// Whether any part of `path`, split at `/` and `\\`, is `.codex` ([`is_codex`]).
pub(super) fn has_codex_part(path: &Path) -> bool {
    path.to_str().is_some_and(text_has_codex_part)
}

/// Whether any part of `text`, split at `/` and `\\`, is `.codex` ([`is_codex`]).
pub(super) fn text_has_codex_part(text: &str) -> bool {
    text.split(['/', '\\']).any(is_codex)
}

/// The files a Codex `apply_patch` patch adds, updates, deletes or moves to.
pub(super) fn patch_targets(patch: &str) -> Vec<PathBuf> {
    const HEADERS: [&str; 4] = [
        "*** Add File:",
        "*** Update File:",
        "*** Delete File:",
        "*** Move to:",
    ];
    patch
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            HEADERS
                .iter()
                .find_map(|header| line.strip_prefix(header))
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// `.codex` is refused in every case and with trailing dots and spaces (it has no `s` or
    /// `k` for the long s or the Kelvin sign to stand for).
    #[test]
    fn a_codex_folder_is_found_in_any_folded_spelling() {
        for name in [".codex", ".CODEX", ".CoDeX", ".codex. "] {
            assert!(super::is_codex(name), "{name}");
        }
        assert!(!super::is_codex(".codexx"));
    }
}
