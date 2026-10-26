//! The exclusion list: names never versioned and never listed. Matched ASCII-case-insensitively
//! on every OS, because the same folder syncs between Windows and macOS.

/// Whole file names that are excluded.
const EXCLUDED_NAMES: [&str; 3] = ["thumbs.db", "desktop.ini", ".ds_store"];

/// Whether a file named `name` is never versioned and never listed: `~$*`, `*.tmp`,
/// `Thumbs.db`, `desktop.ini`, `.DS_Store`, `.~lock.*#`, and the names of
/// [`is_excluded_any`]. ASCII-case-insensitive on every OS.
pub(crate) fn is_excluded_file(name: &str) -> bool {
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
pub(crate) fn is_excluded_any(name: &str) -> bool {
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
