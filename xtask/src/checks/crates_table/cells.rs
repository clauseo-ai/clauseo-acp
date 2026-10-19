//! The names a cell of the Crates table may hold, and how a cell splits into entries.

/// The comma-separated entries of a cell, trimmed, empty ones dropped.
pub(super) fn entries(cell: &str) -> Vec<String> {
    cell.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `^[a-z][a-z0-9_]*$`: a directory under `crates/`.
pub(super) fn is_crate_name(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_lowercase())
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `^[A-Za-z0-9_-]+$`: what crates.io accepts as a package name.
pub(super) fn is_package_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
