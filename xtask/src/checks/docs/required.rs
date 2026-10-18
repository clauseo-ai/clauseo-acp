//! The three files every agent reads first. `AGENTS.md` and `ARCHITECTURE.md` stay within their
//! line and byte limits. `CLAUDE.md` holds the import of `AGENTS.md` and nothing else.

use crate::checks::{self, violation};
use crate::violation::Violation;

/// The agents' rules, read first by every agent.
pub(super) const AGENTS: &str = "AGENTS.md";
/// The map of the repo, with the Crates table.
pub(super) const ARCHITECTURE: &str = "ARCHITECTURE.md";
/// The file Claude Code reads, which imports `AGENTS.md`.
pub(super) const CLAUDE: &str = "CLAUDE.md";

/// The most lines `AGENTS.md` may have.
const AGENTS_LINES: usize = 120;
/// The most bytes `AGENTS.md` may have: 8 KiB.
const AGENTS_BYTES: usize = 8192;
/// The most lines `ARCHITECTURE.md` may have.
const ARCHITECTURE_LINES: usize = 300;
/// The one line `CLAUDE.md` may hold.
const IMPORT: &str = "@AGENTS.md";

/// The violation for a required file that does not exist. A file has no line, so this one is at
/// line 1, the whole-file line (see `Violation`).
pub(super) fn missing(file: &str) -> Violation {
    let fix = match file {
        CLAUDE => {
            "create CLAUDE.md holding only `@AGENTS.md`, or restore it from git history".to_owned()
        }
        _ => format!("create {file} at the repo root, or restore it from git history"),
    };
    violation(file, 1, format!("{file} does not exist"), fix)
}

/// The problems with the text of required file `file`. A file that is not one of the three
/// has none.
pub(super) fn problems(file: &str, text: &str) -> Vec<Violation> {
    match file {
        AGENTS => {
            let limits = [
                over_lines(file, text, AGENTS_LINES),
                over_bytes(file, text, AGENTS_BYTES),
            ];
            limits.into_iter().flatten().collect()
        }
        ARCHITECTURE => over_lines(file, text, ARCHITECTURE_LINES)
            .into_iter()
            .collect(),
        CLAUDE => claude(text).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The violation when `text` has more than `limit` lines, at the first line over the limit.
fn over_lines(file: &str, text: &str, limit: usize) -> Option<Violation> {
    let lines = text.lines().count();
    (lines > limit).then(|| {
        violation(
            file,
            checks::line_at(limit),
            format!("{file} has {lines} lines, over the limit of {limit}"),
            format!("cut {file} to {limit} lines, moving what you cut into the file that owns it"),
        )
    })
}

/// The violation when `text` has more than `limit` bytes, at the line of the first byte over.
fn over_bytes(file: &str, text: &str, limit: usize) -> Option<Violation> {
    let bytes = text.len();
    (bytes > limit).then(|| {
        violation(
            file,
            line_of_byte(text, limit),
            format!("{file} is {bytes} bytes, over the limit of {limit}"),
            format!("cut {file} to {limit} bytes, moving what you cut into the file that owns it"),
        )
    })
}

/// The 1-based line that holds the byte at `index`.
fn line_of_byte(text: &str, index: usize) -> u32 {
    let newlines = text
        .as_bytes()
        .iter()
        .take(index)
        .filter(|byte| **byte == b'\n')
        .count();
    checks::line_at(newlines)
}

/// The violation when `CLAUDE.md` is not exactly `@AGENTS.md`, with or without its final newline.
/// It points at line 2 when line 1 is the import and text follows it, else at line 1.
fn claude(text: &str) -> Option<Violation> {
    if text == IMPORT || text == "@AGENTS.md\n" {
        return None;
    }
    let mut lines = text.lines();
    let import_first = lines.next() == Some(IMPORT);
    let line = if import_first && lines.next().is_some() {
        2
    } else {
        1
    };
    Some(violation(
        CLAUDE,
        line,
        "CLAUDE.md must hold only `@AGENTS.md`",
        "move what CLAUDE.md holds besides `@AGENTS.md` into AGENTS.md, then keep only `@AGENTS.md` in CLAUDE.md",
    ))
}

#[cfg(test)]
mod tests;
