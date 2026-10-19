//! The `file-size` check: every Rust and TypeScript file in the repo (`.rs`, `.ts`, `.tsx`, `.mts`,
//! `.cts`, in any case) has at most 600 lines, unless `xtask/file-size-exceptions.txt` lists it,
//! and every comment in a `.rs` file stays within 100 columns. A changed file of 501 to 600 lines
//! gets a note, which never fails.

mod lexer;

use crate::changes;
use crate::checks::exceptions::{self, Exception};
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::collections::BTreeSet;
use std::path::Path;

/// The most lines a source file may have, unless it is an exception.
const LINE_LIMIT: usize = 600;
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
    place: "in the repo's .rs, .ts, .tsx, .mts and .cts files",
    source: "xtask/src/checks/file_size.rs",
    run,
};

/// Lists the repo's source files and inspects each one. Not a git repo: nothing is listed, so
/// nothing is inspected and the runner reports the zero.
fn run(ctx: &Context) -> Inspection {
    let Ok(files) = changes::repo_files(&ctx.root) else {
        return Inspection::default();
    };
    let sources: Vec<String> = files.into_iter().filter(|file| is_source(file)).collect();
    let (exceptions, mut violations) = exceptions::load(&ctx.root, EXCEPTIONS);
    let exempt: BTreeSet<&str> = exceptions.iter().map(|entry| entry.path.as_str()).collect();
    let mut sizes = Vec::new();
    for file in &sources {
        match checks::read_repo_file(&ctx.root, file) {
            Ok(Some(text)) => {
                let lines = text.lines().count();
                sizes.push((file.clone(), lines));
                violations.extend(size_violation(file, lines, exempt.contains(file.as_str())));
                if is_rust(file) {
                    violations.extend(comment_violations(file, &text));
                }
            }
            Ok(None) => {}
            Err(violation) => violations.push(violation),
        }
    }
    violations.extend(stale_exceptions(&exceptions, &sources, &sizes));
    Inspection {
        inspected: sources.len(),
        violations,
        notes: notes(&ctx.root, &sizes),
    }
}

/// True for the files this check reads: `.rs`, `.ts`, `.tsx`, `.mts` and `.cts`, in any case.
fn is_source(file: &str) -> bool {
    matches!(
        extension(file).as_deref(),
        Some("rs" | "ts" | "tsx" | "mts" | "cts")
    )
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

crate::checks::planted::cases! {
    /// A `.rs` file of 601 lines fails at line 1 and names the limit and the two fixes.
    rs_601_lines: FailsWith(&[
        "src/big.rs:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
    ]) => tests::rs_601_lines,
    /// A `.ts` file of 601 lines fails the same way.
    ts_601_lines: FailsWith(&[
        "ui/big.ts:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
    ]) => tests::ts_601_lines,
    /// A `.tsx` file of 601 lines fails the same way.
    tsx_601_lines: FailsWith(&[
        "ui/Big.tsx:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
    ]) => tests::tsx_601_lines,
    /// A `.mts` file of 601 lines fails the same way, and so does an upper-case `.RS` file.
    mts_and_upper_case_rs_601_lines: FailsWith(&[
        "src/LIB.RS:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
        "ui/big.mts:1: this file has 601 lines; the limit is 600\n    \
         fix: split it into smaller modules, or list it in xtask/file-size-exceptions.txt with a reason",
    ]) => tests::mts_and_upper_case_rs_601_lines,
    /// An exception for a path that no longer exists fails at its line in the exception file.
    stale_exception_gone: FailsWith(&[
        "xtask/file-size-exceptions.txt:2: `src/gone.rs` is listed as an exception, but the \
         check inspects no Rust or TypeScript file at that path\n    \
         fix: delete this line from xtask/file-size-exceptions.txt",
    ]) => tests::stale_exception_gone,
    /// An exception for a file of 450 lines fails: it needs no exception.
    stale_exception_short: FailsWith(&[
        "xtask/file-size-exceptions.txt:2: `src/short.rs` is listed as an exception, but it has \
         450 lines, within the 600-line limit\n    \
         fix: delete this line from xtask/file-size-exceptions.txt",
    ]) => tests::stale_exception_short,
    /// A line comment that reaches column 101 fails, and the message names the column.
    line_comment_101: FailsWith(&[
        "src/lib.rs:2: this comment reaches column 101; the limit is 100\n    \
         fix: wrap the comment at 100 columns",
    ]) => tests::line_comment_101,
    /// A doc comment (`//!`) that reaches column 101 fails.
    doc_comment_101: FailsWith(&[
        "src/lib.rs:1: this comment reaches column 101; the limit is 100\n    \
         fix: wrap the comment at 100 columns",
    ]) => tests::doc_comment_101,
    /// A comment after code whose last column is 102 fails, on the code's line.
    trailing_comment_past_100: FailsWith(&[
        "src/lib.rs:1: this comment reaches column 102; the limit is 100\n    \
         fix: wrap the comment at 100 columns",
    ]) => tests::trailing_comment_past_100,
    /// A block comment whose second physical line reaches past column 100 fails on that line.
    block_comment_past_100: FailsWith(&[
        "src/lib.rs:2: this comment reaches column 110; the limit is 100\n    \
         fix: wrap the comment at 100 columns",
    ]) => tests::block_comment_past_100,
    /// A comment of 100 characters, `//` included, whose rest is `é`, passes.
    e_acute_comment_100_passes: Passes => tests::e_acute_comment_100_passes,
    /// A code line with a 101-character string literal passes: code is exempt.
    string_literal_101_passes: Passes => tests::string_literal_101_passes,
    /// A 101-character string literal that holds `//` passes: it is not a comment.
    string_holding_slashes_101_passes: Passes => tests::string_holding_slashes_101_passes,
    /// A raw string (`r#"…"#`) holding `//`, on one line and across lines, passes.
    raw_string_holding_slashes_passes: Passes => tests::raw_string_holding_slashes_passes,
    /// A `'"'` char literal does not open a string, so the `//` in a later string is still
    /// inside a string and passes.
    char_quote_then_code_passes: Passes => tests::char_quote_then_code_passes,
    /// Lifetimes and labels are not char literals, so the string after them passes.
    lifetime_then_string_passes: Passes => tests::lifetime_then_string_passes,
    /// A file of 650 lines listed in the exception file, with a reason, passes.
    listed_600_plus_file_passes: Passes => tests::listed_600_plus_file_passes,
}

#[cfg(test)]
mod tests;
