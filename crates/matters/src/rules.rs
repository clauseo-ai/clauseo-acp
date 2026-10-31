//! The exclusion list: names never versioned and never listed. Matched ASCII-case-insensitively
//! on every OS, because the same folder syncs between Windows and macOS.

/// Whether the walk skips an entry named `name`, a folder when `is_folder` (with everything
/// under it): the one rule the walk and every write into the folder apply.
pub(crate) fn is_excluded(name: &str, is_folder: bool) -> bool {
    if is_folder {
        is_excluded_any(name)
    } else {
        is_excluded_file(name)
    }
}

/// Whether the walk skips the file at `path` (`/`-separated, relative to the matter folder):
/// a folder on the way or the file itself is excluded by [`is_excluded`].
pub(crate) fn is_excluded_path(path: &str) -> bool {
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        if is_excluded(part, parts.peek().is_some()) {
            return true;
        }
    }
    false
}

/// Whole file names that are excluded.
const EXCLUDED_NAMES: [&str; 3] = ["thumbs.db", "desktop.ini", ".ds_store"];

/// Whether a file named `name` is never versioned and never listed: `~$*`, `*.tmp`,
/// `Thumbs.db`, `desktop.ini`, `.DS_Store`, `.~lock.*#`, and the names of
/// [`is_excluded_any`]. ASCII-case-insensitive on every OS.
fn is_excluded_file(name: &str) -> bool {
    is_excluded_any(name)
        || starts_with_ignore_case(name, "~$")
        || ends_with_ignore_case(name, ".tmp")
        || EXCLUDED_NAMES
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(name))
        // The prefix does not end in `#`, so a name ending in one is longer than the prefix.
        || (starts_with_ignore_case(name, ".~lock.") && name.ends_with('#'))
}

/// Whether a folder (or file) named `name` is skipped with everything under it: `.git` and
/// `.clauseo`, in any ASCII case.
fn is_excluded_any(name: &str) -> bool {
    name.eq_ignore_ascii_case(".git") || name.eq_ignore_ascii_case(".clauseo")
}

fn starts_with_ignore_case(name: &str, prefix: &str) -> bool {
    name.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

fn ends_with_ignore_case(name: &str, suffix: &str) -> bool {
    name.len()
        .checked_sub(suffix.len())
        .and_then(|start| name.as_bytes().get(start..))
        .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix.as_bytes()))
}

#[cfg(test)]
mod tests;
