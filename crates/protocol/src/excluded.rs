//! The exclusion list: names a matter's history never versions and the papers list never shows.
//! Matched ASCII-case-insensitively on every OS, because the same folder syncs between Windows
//! and macOS. It lives here so `clauseo-matters` (the walk, every write) and the reducer (the
//! files of turns logged before a rule was added) apply the same one.

/// Whether the walk skips an entry named `name`, a folder when `is_folder` (with everything
/// under it): the one rule the walk and every write into the folder apply.
#[must_use]
pub fn is_excluded_name(name: &str, is_folder: bool) -> bool {
    if is_folder {
        is_excluded_folder(name)
    } else {
        is_excluded_file(name)
    }
}

/// Whether the walk skips the file at `path` (`/`-separated, relative to the matter folder):
/// a folder on the way or the file itself is excluded by [`is_excluded_name`]. The host's watch
/// of an open matter's folder passes over a change to such a file: the listing never shows it.
/// A history or a logged turn that holds such a path (versioned before the rule) shows it
/// nowhere: no change, no deletion, nothing for undo or restore to write.
#[must_use]
pub fn is_excluded_path(path: &str) -> bool {
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        if is_excluded_name(part, parts.peek().is_some()) {
            return true;
        }
    }
    false
}

/// Whole file names that are excluded.
const EXCLUDED_NAMES: [&str; 3] = ["thumbs.db", "desktop.ini", ".ds_store"];

/// Whether a file named `name` is never versioned and never listed: `~$*`, `*.tmp`,
/// `Thumbs.db`, `desktop.ini`, `.DS_Store`, `.~lock.*#`, and a file named `.git` or
/// `.clauseo`. ASCII-case-insensitive on every OS. Other dot-files (`.gitignore`) are kept.
fn is_excluded_file(name: &str) -> bool {
    name.eq_ignore_ascii_case(".git")
        || name.eq_ignore_ascii_case(".clauseo")
        || starts_with_ignore_case(name, "~$")
        || ends_with_ignore_case(name, ".tmp")
        || EXCLUDED_NAMES
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(name))
        // The prefix does not end in `#`, so a name ending in one is longer than the prefix.
        || (starts_with_ignore_case(name, ".~lock.") && name.ends_with('#'))
}

/// Whether a folder named `name` is skipped with everything under it: any name that starts
/// with `.` (`.git`, `.clauseo`, and the folders agents keep their settings in, such as
/// `.claude`, `.codex` and `.agents`), hidden on macOS and Linux and never a paper.
fn is_excluded_folder(name: &str) -> bool {
    name.starts_with('.')
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
