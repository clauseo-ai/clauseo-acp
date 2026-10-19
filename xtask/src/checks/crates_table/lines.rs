//! The line-level reading of the Crates table: which lines belong to it, how a line splits into
//! cells and a cell into entries, and the names a cell may hold.

use super::{FILE, SECTION};
use crate::checks::violation;
use crate::violation::Violation;

/// The violation for line `line` (text `text`) of the `## Crates` section, the first non-blank
/// line after the table (whose last line is `end`), which holds a `|`.
pub(super) fn past_the_end(end: u32, line: u32, text: &str) -> Violation {
    let fix = if is_table_line(text) {
        format!(
            "remove the blank lines between line {end} and line {line}, so the row joins the table, or move the text at line {line} out of the `{SECTION}` section: a blank line ends the table"
        )
    } else {
        format!(
            "start line {line} with `|` if it is a row (Markdown reads a row without a leading pipe, xtask/src/checks/crates_table.rs does not), else write it without a `|`"
        )
    };
    violation(
        FILE,
        line,
        format!(
            "the Crates table ends at line {end}, but line {line} after it holds a `|`, so the rows from there on are not read"
        ),
        fix,
    )
}

/// True for a line of a pipe table: it starts with `|` after any indent.
pub(super) fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

/// The cells of a table line: split on `|` (not `\|`, which is a literal pipe), the outer pipes
/// dropped, each cell trimmed and its backticks removed.
pub(super) fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let inner = line.strip_prefix('|').unwrap_or(line);
    let inner = match inner.strip_suffix('|') {
        Some(rest) if !rest.ends_with('\\') => rest,
        _ => inner,
    };
    let mut found = Vec::new();
    let mut cell = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => found.push(std::mem::take(&mut cell)),
            '`' => {}
            _ => cell.push(c),
        }
    }
    found.push(cell);
    found.iter().map(|cell| cell.trim().to_owned()).collect()
}

/// The comma-separated entries of a cell, trimmed, empty ones dropped.
pub(super) fn entries(cell: &str) -> Vec<String> {
    cell.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

/// True for a separator cell: `---`, with an optional `:` at either end.
pub(super) fn is_rule(cell: &str) -> bool {
    let dashes = cell.trim_start_matches(':').trim_end_matches(':');
    !dashes.is_empty() && dashes.chars().all(|c| c == '-')
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
