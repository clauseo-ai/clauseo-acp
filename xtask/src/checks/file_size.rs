//! The `file-size` check: every Rust and TypeScript file in the repo (`.rs`, `.ts`, `.tsx`, `.mts`,
//! `.cts`, in any case) has at most 600 lines, unless `xtask/file-size-exceptions.txt` lists it,
//! and every comment in a `.rs` file stays within 100 columns. A changed file of 501 to 600 lines
//! gets a note, which never fails.
//!
//! The files come from `git ls-files --cached --others --exclude-standard` (`changes::repo_files`):
//! tracked files, and also untracked files that `.gitignore` does not exclude. The untracked ones
//! are read on purpose: the check catches a new file before anyone adds it, and it sees what
//! `--quick` sees. Do not tighten it to tracked files only. When git cannot list the files, the
//! check fails with git's error. It counts the files it read.

mod fix;
pub(crate) use fix::Fix;

mod lexer;

use crate::changes;
use crate::checks::exceptions::{self, Exception};
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::collections::BTreeSet;
use std::path::Path;

/// The most lines a source file may have, unless it is an exception.
pub(super) const LINE_LIMIT: usize = 600;
/// A changed file over this many lines gets a note.
const NOTE_FROM: usize = 500;
/// The last column a comment may reach, in characters.
const COLUMN_LIMIT: usize = 100;
/// The exception file, repo-relative. Its entries are `<path> <reason>`.
const EXCEPTIONS: &str = "xtask/file-size-exceptions.txt";

/// Fails on a source file over 600 lines that is not an exception, on a stale exception, and on
/// a comment in a `.rs` file that reaches past column 100.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "file-size",
    counts: Noun {
        one: "source file",
        many: "source files",
    },
    place: || {
        let (last, rest) = SOURCE_EXTENSIONS.split_last().unwrap_or((&"", &[]));
        format!("in the repo's .{} and .{last} files", rest.join(", ."))
    },
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// The extensions of the files this check reads, matched in any case.
const SOURCE_EXTENSIONS: [&str; 5] = ["rs", "ts", "tsx", "mts", "cts"];

/// Lists the repo's source files and inspects each one. When git cannot list them, the failure
/// says git's error.
fn run(ctx: &Context) -> Inspection {
    let files = match changes::repo_files(&ctx.root) {
        Ok(files) => files,
        Err(error) => return checks::unlisted(CHECK.name, &error),
    };
    let sources: Vec<String> = files.into_iter().filter(|file| is_source(file)).collect();
    let (exceptions, mut violations) = exceptions::load(&ctx.root, EXCEPTIONS);
    let exempt: BTreeSet<&str> = exceptions.iter().map(|entry| entry.path.as_str()).collect();
    let mut sizes = Vec::new();
    let mut read = 0;
    let mut unreadable = 0;
    for file in &sources {
        match checks::read_repo_file(&ctx.root, file) {
            Ok(Some(text)) => {
                read += 1;
                let lines = text.lines().count();
                sizes.push((file.clone(), lines));
                violations.extend(size_violation(file, lines, exempt.contains(file.as_str())));
                if is_rust(file) {
                    violations.extend(comment_violations(file, &text));
                }
            }
            Ok(None) => {}
            Err(violation) => {
                unreadable += 1;
                violations.push(violation);
            }
        }
    }
    violations.extend(stale_exceptions(&exceptions, &sources, &sizes));
    // Source files that are there but could not be read, and none that could: the check found
    // its input and stopped on it, which the read failures above say.
    let stopped = read == 0 && unreadable > 0;
    Inspection {
        inspected: read,
        violations,
        failures: Vec::new(),
        notes: notes(&ctx.root, &sizes),
        stopped,
    }
}

/// True for the files this check reads: one of `SOURCE_EXTENSIONS`, in any case.
fn is_source(file: &str) -> bool {
    extension(file).is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext.as_str()))
}

/// True for a `.rs` file, the only kind whose comments are checked.
fn is_rust(file: &str) -> bool {
    extension(file).as_deref() == Some("rs")
}

/// The extension of `file` in lowercase, if it has one, so `lib.RS` counts as Rust.
fn extension(file: &str) -> Option<String> {
    Path::new(file)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
}

/// The violation for a file of `lines` lines that is over the limit and not exempt, at line 1.
fn size_violation(file: &str, lines: usize, exempt: bool) -> Option<Violation> {
    (lines > LINE_LIMIT && !exempt).then(|| {
        checks::violation(
            file,
            1,
            format!("this file has {lines} lines; the limit is {LINE_LIMIT}"),
            format!("split it into smaller modules, or list it in {EXCEPTIONS} with a reason"),
        )
    })
}

/// One violation for each physical line on which a comment reaches past the column limit.
fn comment_violations(file: &str, text: &str) -> Vec<Violation> {
    lexer::reaches(text)
        .into_iter()
        .filter(|reach| reach.column > COLUMN_LIMIT)
        .map(|reach| {
            checks::violation(
                file,
                reach.line,
                format!(
                    "this comment reaches column {}; the limit is {COLUMN_LIMIT}",
                    reach.column
                ),
                format!("wrap the comment at {COLUMN_LIMIT} columns"),
            )
        })
        .collect()
}

/// The violations for exceptions that no longer earn their line, each at its line in `EXCEPTIONS`.
fn stale_exceptions(
    exceptions: &[Exception],
    sources: &[String],
    sizes: &[(String, usize)],
) -> Vec<Violation> {
    let fix = format!("delete this line from {EXCEPTIONS}");
    exceptions
        .iter()
        .filter_map(|entry| {
            stale_message(entry, sources, sizes)
                .map(|message| checks::violation(EXCEPTIONS, entry.line, message, fix.as_str()))
        })
        .collect()
}

/// Why `entry` is stale, if it is: its path is not a source file, or its file is within the
/// limit. A source file that could not be read is not called stale; its own violation says why.
fn stale_message(
    entry: &Exception,
    sources: &[String],
    sizes: &[(String, usize)],
) -> Option<String> {
    if !sources.contains(&entry.path) {
        return Some(format!(
            "`{}` is listed as an exception, but the check inspects no Rust or TypeScript file at \
             that path",
            entry.path
        ));
    }
    let (_, lines) = sizes.iter().find(|(file, _)| *file == entry.path)?;
    (*lines <= LINE_LIMIT).then(|| {
        format!(
            "`{}` is listed as an exception, but it has {lines} lines, within the {LINE_LIMIT}-line \
             limit",
            entry.path
        )
    })
}

/// The notes for the changed files, or one note saying why there are none.
fn notes(root: &Path, sizes: &[(String, usize)]) -> Vec<String> {
    match changes::changed_files(root) {
        Ok(changed) => near_limit_notes(&changed, sizes),
        Err(error) => vec![format!(
            "no near-limit notes, since the changed files are unknown: {error}"
        )],
    }
}

/// `<file> has <n> of 600 lines; split it now` for each changed file of more than 500 lines and
/// at most 600. A file over 600 is a failure, not a note, so it gets none here.
fn near_limit_notes(changed: &[String], sizes: &[(String, usize)]) -> Vec<String> {
    let changed: BTreeSet<&str> = changed.iter().map(String::as_str).collect();
    sizes
        .iter()
        .filter(|(file, lines)| {
            changed.contains(file.as_str()) && *lines > NOTE_FROM && *lines <= LINE_LIMIT
        })
        .map(|(file, lines)| format!("{file} has {lines} of {LINE_LIMIT} lines; split it now"))
        .collect()
}

#[cfg(test)]
mod tests;
