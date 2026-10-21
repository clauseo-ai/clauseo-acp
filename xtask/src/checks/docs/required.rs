//! The files every agent reads first, listed in `REQUIRED`. `AGENTS.md` and `ARCHITECTURE.md`
//! stay within their line and byte limits. `CLAUDE.md` holds the import of `AGENTS.md` and
//! nothing else.

use super::Fix;
use crate::checks::{self, violation};
use crate::violation::Violation;

/// A file every agent reads first, and the rule it keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Required {
    /// Its path from the repo root.
    pub file: &'static str,
    pub rule: Rule,
}

/// What a required file must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Rule {
    /// At most `lines` lines and, when set, at most `bytes` bytes.
    Limits { lines: usize, bytes: Option<usize> },
    /// Exactly `@<file>`, the import of another file, with or without its final newline.
    Import(&'static str),
}

/// The required files: the one place their names and limits live. A new rule is a new row.
pub(super) const REQUIRED: [Required; 3] = [
    // The agents' rules, read first by every agent. 8 KiB.
    Required {
        file: "AGENTS.md",
        rule: Rule::Limits {
            lines: 120,
            bytes: Some(8192),
        },
    },
    // The map of the repo, with the Crates table.
    Required {
        file: "ARCHITECTURE.md",
        rule: Rule::Limits {
            lines: 300,
            bytes: None,
        },
    },
    // The file Claude Code reads, which imports `AGENTS.md`.
    Required {
        file: "CLAUDE.md",
        rule: Rule::Import("AGENTS.md"),
    },
];

/// The violation for a required file that does not exist. A file has no line, so this one is at
/// line 1, the whole-file line (see `Violation`).
pub(super) fn missing(required: &Required) -> Violation {
    let file = required.file;
    let fix = match required.rule {
        Rule::Import(target) => Fix::CreateImport {
            file: file.to_owned(),
            target: target.to_owned(),
        },
        Rule::Limits { .. } => Fix::CreateRequired {
            file: file.to_owned(),
        },
    };
    violation(file, 1, format!("{file} does not exist"), fix)
}

/// The problems with `text`, the contents of the required file.
pub(super) fn problems(required: &Required, text: &str) -> Vec<Violation> {
    let file = required.file;
    match required.rule {
        Rule::Limits { lines, bytes } => {
            let over = [
                over_lines(file, text, lines),
                bytes.and_then(|limit| over_bytes(file, text, limit)),
            ];
            over.into_iter().flatten().collect()
        }
        Rule::Import(target) => import(file, target, text).into_iter().collect(),
    }
}

/// The violation when `text` has more than `limit` lines, at the first line over the limit.
fn over_lines(file: &str, text: &str, limit: usize) -> Option<Violation> {
    let lines = checks::line_count(text.as_bytes());
    (lines > limit).then(|| {
        violation(
            file,
            checks::line_at(limit),
            format!("{file} has {lines} lines, over the limit of {limit}"),
            Fix::CutLines {
                file: file.to_owned(),
                limit,
            },
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
            Fix::CutBytes {
                file: file.to_owned(),
                limit,
            },
        )
    })
}

/// The 1-based line that holds the byte at `index`, with lines ended as `checks::line_count`
/// ends them: the count of lines up to that byte, itself included.
fn line_of_byte(text: &str, index: usize) -> u32 {
    let through = text.as_bytes().get(..=index).unwrap_or(text.as_bytes());
    u32::try_from(checks::line_count(through).max(1)).unwrap_or(u32::MAX)
}

/// The violation when `file` is not exactly `@<target>`, with or without its final newline. It
/// points at line 2 when line 1 is the import and text follows it, else at line 1.
fn import(file: &str, target: &str, text: &str) -> Option<Violation> {
    let line = format!("@{target}");
    if text == line || text.strip_suffix('\n') == Some(line.as_str()) {
        return None;
    }
    let mut lines = text.lines();
    let import_first = lines.next() == Some(line.as_str());
    let at = if import_first && lines.next().is_some() {
        2
    } else {
        1
    };
    Some(violation(
        file,
        at,
        format!("{file} must hold only `{line}`"),
        Fix::MoveImport {
            file: file.to_owned(),
            target: target.to_owned(),
        },
    ))
}

#[cfg(test)]
mod tests;
