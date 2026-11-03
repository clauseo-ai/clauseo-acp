//! The Codex side of the guard: a patch's targets, and the matter's `.codex` folder
//! (clauseo-acp-private#184).

use std::path::{Path, PathBuf};

use super::{existing_resolved, lexical};

/// Whether `path`, taken from `base`, is `<folder>/.codex` or lies under it, resolved
/// canonically or lexically.
pub(super) fn in_codex_folder(folder: &Path, base: &Path, path: &Path) -> bool {
    let absolute = base.join(path);
    let root = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    if let Some(inside) = clauseo_matters::matter_path_in(&root, &existing_resolved(&absolute))
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

/// Whether `name` is `.codex`, ignoring ASCII case and trailing dots and spaces.
fn is_codex(name: &str) -> bool {
    name.trim_end_matches(['.', ' '])
        .eq_ignore_ascii_case(".codex")
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
