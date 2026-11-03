//! Quoted search patterns, which the wildcard rule ([`super::globbed`]) leaves alone: `find
//! -name '*.docx'`, `rg -g '*.docx'`, `grep --include='*.docx'`, PowerShell `Get-ChildItem
//! -Filter '*.docx'`. Such a pattern is matched by the program, not expanded by the shell into
//! file operands, so it names no file the command writes.

use super::program_name;
use crate::pre_write_guard::shell::command_words;
use crate::pre_write_guard::words::Segment;

/// `find` actions that write or run something; a `find` with one keeps the wildcard rule.
const FIND_ACTIONS: [&str; 9] = [
    "-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprint0", "-fprintf", "-fls",
];

/// The programs a pattern command may pipe into, all reading their input only.
const PIPE_READERS: [&str; 11] = [
    "head", "tail", "wc", "sort", "uniq", "grep", "cut", "tr", "nl", "column", "cat",
];

/// The program of the simple command `segment`, as [`program_name`] gives it.
fn program(segment: &Segment) -> Option<String> {
    command_words(&segment.words)
        .first()
        .map(|word| program_name(word))
}

/// Whether `segment` is one of the search commands whose patterns the wildcard rule leaves
/// alone: `find` with no action that writes or runs, `rg`, `grep`, or PowerShell's
/// `Get-ChildItem` (`gci`, `dir`).
fn searches(segment: &Segment) -> bool {
    match program(segment).as_deref() {
        Some("find") => !segment
            .words
            .iter()
            .any(|word| FIND_ACTIONS.contains(&word.as_str())),
        Some("rg" | "grep" | "get-childitem" | "gci" | "dir") => true,
        _ => false,
    }
}

/// Whether every simple command of `segments` (heredoc bodies aside) is a search command
/// ([`searches`]) or a reader of its input ([`PIPE_READERS`]), with at least one search: only
/// then may a quoted pattern stay out of the wildcard rule, since nothing else in the command
/// (`| xargs rm`, `| Remove-Item`) acts on what the search finds.
pub(super) fn only_searches(segments: &[&Segment]) -> bool {
    let commands = || segments.iter().copied().filter(|segment| segment.command);
    commands().any(searches)
        && commands().all(|segment| {
            searches(segment)
                || program(segment).is_some_and(|name| PIPE_READERS.contains(&name.as_str()))
        })
}

/// Whether word `at` of the search command `segment` is a quoted pattern its program matches:
/// after `find`'s `-name`, `-iname`, `-path`, `-ipath`, `-wholename`, `-iwholename`; `rg`'s
/// `-g`, `--glob`, `--iglob` (or `--glob=`); `grep`'s `--include`, `--exclude` (or with `=`);
/// PowerShell's `-Filter` or `-Include` (any case).
pub(super) fn quoted_pattern(segment: &Segment, at: usize) -> bool {
    if !segment.quoted.get(at).copied().unwrap_or(false) || !searches(segment) {
        return false;
    }
    let Some(word) = segment.words.get(at) else {
        return false;
    };
    let previous = at
        .checked_sub(1)
        .and_then(|before| segment.words.get(before))
        .map(String::as_str)
        .unwrap_or_default();
    match program(segment).as_deref() {
        Some("find") => matches!(
            previous,
            "-name" | "-iname" | "-path" | "-ipath" | "-wholename" | "-iwholename"
        ),
        Some("rg") => {
            matches!(previous, "-g" | "--glob" | "--iglob")
                || word.starts_with("--glob=")
                || word.starts_with("--iglob=")
        }
        Some("grep") => {
            matches!(previous, "--include" | "--exclude")
                || word.starts_with("--include=")
                || word.starts_with("--exclude=")
        }
        Some(_) => matches!(
            previous.to_ascii_lowercase().as_str(),
            "-filter" | "-include"
        ),
        None => false,
    }
}
