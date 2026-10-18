//! The `docs` check: the Markdown files of the repo, read by hand. In every file, relative links
//! resolve, backticked repo paths exist and `cargo xtask` commands parse against the clap tree.
//! `AGENTS.md`, `ARCHITECTURE.md` and `CLAUDE.md` also keep their limits and their content. It
//! counts the Markdown files it read.

mod commands;
mod links;
mod markdown;
mod paths;
mod required;

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;

use crate::changes;
use crate::checks::crates_table::{self, Status};
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::path::Path;

/// Fails on every broken link, missing path, unparsed `cargo xtask` command and required-file
/// rule in the Markdown files of the repo.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "docs",
    counts: Noun {
        one: "Markdown file",
        many: "Markdown files",
    },
    place: "in the repo's git-tracked or unignored *.md files",
    run,
};

fn run(ctx: &Context) -> Inspection {
    match changes::repo_files(&ctx.root) {
        Ok(files) => {
            let markdown: Vec<String> = files
                .into_iter()
                .filter(|file| Path::new(file).extension().is_some_and(|ext| ext == "md"))
                .collect();
            inspect(&ctx.root, &markdown)
        }
        // Not a git repo, so there are no files to read. The runner reports the zero.
        Err(_) => Inspection::default(),
    }
}

/// Checks the Markdown files `markdown` (repo-relative) under `root`, and the required files.
/// Counts each Markdown file it could read.
fn inspect(root: &Path, markdown: &[String]) -> Inspection {
    let planned = planned_rows(root);
    let mut inspected = 0;
    let mut violations = Vec::new();
    for file in markdown {
        match checks::read_repo_file(root, file) {
            Ok(Some(text)) => {
                inspected += 1;
                violations.extend(file_problems(root, file, &text, &planned));
            }
            Ok(None) => {}
            Err(unreadable) => {
                inspected += 1;
                violations.push(unreadable);
            }
        }
    }
    violations.extend(required_problems(root));
    violations.sort_by(|a, b| (a.path.as_str(), a.line).cmp(&(b.path.as_str(), b.line)));
    violations.dedup();
    Inspection::new(inspected, violations)
}

/// The names of the Crates-table rows whose status is `planned`. A table that does not read
/// yields none: its own check reports why.
fn planned_rows(root: &Path) -> Vec<String> {
    crates_table::load(root).map_or_else(
        |_| Vec::new(),
        |table| {
            table
                .rows
                .into_iter()
                .filter(|row| row.status == Status::Planned)
                .map(|row| row.name)
                .collect()
        },
    )
}

/// Every problem in one Markdown file: its links, its repo paths and its xtask commands.
fn file_problems(root: &Path, file: &str, text: &str, planned: &[String]) -> Vec<Violation> {
    let scan = markdown::scan(text);
    let mut found = links::problems(root, file, &scan.links);
    found.extend(paths::problems(root, file, &scan.spans, planned));
    found.extend(commands::problems(file, &scan));
    found
}

/// The problems with the three required files: missing, over a limit, or not as they must read.
fn required_problems(root: &Path) -> Vec<Violation> {
    [required::AGENTS, required::ARCHITECTURE, required::CLAUDE]
        .into_iter()
        .flat_map(|file| match checks::read_repo_file(root, file) {
            Ok(Some(text)) => required::problems(file, &text),
            Ok(None) => vec![required::missing(file)],
            Err(unreadable) => vec![unreadable],
        })
        .collect()
}

crate::checks::planted::cases! {
    /// A backticked repo path that does not exist fails at the span's line.
    missing_backticked_path: FailsWith(&[
        "GUIDE.md:1: `crates/nope/src/lib.rs` does not exist\n    \
         fix: create `crates/nope/src/lib.rs`, or correct or remove the reference",
    ]) => cases::missing_backticked_path,
    /// A relative link to a missing file fails, naming the file it resolves to.
    broken_relative_link: FailsWith(&[
        "GUIDE.md:1: the relative link `notes.md` does not resolve: `notes.md` does not exist\n    \
         fix: create `notes.md`, or correct or remove the link",
    ]) => cases::broken_relative_link,
    /// The anchor is cut before the path is resolved, so `nope.md#x` fails on `nope.md`.
    missing_file_with_anchor: FailsWith(&[
        "GUIDE.md:1: the relative link `nope.md` does not resolve: `nope.md` does not exist\n    \
         fix: create `nope.md`, or correct or remove the link",
    ]) => cases::missing_file_with_anchor,
    /// A reference definition is a link too, and fails at its own line.
    broken_reference_definition: FailsWith(&[
        "GUIDE.md:3: the relative link `gone.md` does not resolve: `gone.md` does not exist\n    \
         fix: create `gone.md`, or correct or remove the link",
    ]) => cases::broken_reference_definition,
    /// A relative link that climbs out of the repo, other than into the private folder, fails.
    link_leaves_the_repo: FailsWith(&[
        "GUIDE.md:1: the relative link `../../outside.md` leaves the repo\n    \
         fix: point it at a file inside the repo, or remove it",
    ]) => cases::link_leaves_the_repo,
    /// A code span inside an HTML comment counts like any other.
    path_in_html_comment: FailsWith(&[
        "GUIDE.md:1: `crates/gone/` does not exist\n    \
         fix: create `crates/gone/`, or correct or remove the reference",
    ]) => cases::path_in_html_comment,
    /// A span opened by two backticks counts like one opened by a single backtick.
    double_backtick_span: FailsWith(&[
        "GUIDE.md:1: `crates/gone/x.rs` does not exist\n    \
         fix: create `crates/gone/x.rs`, or correct or remove the reference",
    ]) => cases::double_backtick_span,
    /// A crate path whose row is not in the Crates table is not exempt.
    unplanned_crate_path: FailsWith(&[
        "GUIDE.md:1: `crates/store/src/lib.rs` does not exist\n    \
         fix: create `crates/store/src/lib.rs`, or correct or remove the reference",
    ]) => cases::unplanned_crate_path,
    /// An `xtask` subcommand that clap does not know fails at its line, with clap's first line.
    unknown_xtask_subcommand: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => cases::unknown_xtask_subcommand,
    /// An unknown flag fails, as an unknown subcommand does.
    unknown_xtask_flag: FailsWith(&[
        "GUIDE.md:1: the xtask command `cargo xtask check --frobnicate` does not parse: error: unexpected argument '--frobnicate' found\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => cases::unknown_xtask_flag,
    /// A fenced line that starts with `cargo xtask` is checked the same way.
    unknown_xtask_command_in_fence: FailsWith(&[
        "GUIDE.md:2: the xtask command `cargo xtask frobnicate` does not parse: error: unrecognized subcommand 'frobnicate'\n    \
         fix: correct it to a command that `cargo xtask --help` lists (`cargo xtask <command> --help` lists its flags), or remove it",
    ]) => cases::unknown_xtask_command_in_fence,
    /// `AGENTS.md` at 121 lines fails at line 121, the first line over the limit.
    agents_121_lines: FailsWith(&[
        "AGENTS.md:121: AGENTS.md has 121 lines, over the limit of 120\n    \
         fix: cut AGENTS.md to 120 lines, moving what you cut into the file that owns it",
    ]) => cases::agents_121_lines,
    /// `AGENTS.md` at 8193 bytes fails at the line of the 8193rd byte.
    agents_8193_bytes: FailsWith(&[
        "AGENTS.md:1: AGENTS.md is 8193 bytes, over the limit of 8192\n    \
         fix: cut AGENTS.md to 8192 bytes, moving what you cut into the file that owns it",
    ]) => cases::agents_8193_bytes,
    /// `ARCHITECTURE.md` at 301 lines fails at line 301.
    architecture_301_lines: FailsWith(&[
        "ARCHITECTURE.md:301: ARCHITECTURE.md has 301 lines, over the limit of 300\n    \
         fix: cut ARCHITECTURE.md to 300 lines, moving what you cut into the file that owns it",
    ]) => cases::architecture_301_lines,
    /// `CLAUDE.md` with anything after `@AGENTS.md` fails at line 2.
    claude_extra_line: FailsWith(&[
        "CLAUDE.md:2: CLAUDE.md must hold only `@AGENTS.md`\n    \
         fix: move what CLAUDE.md holds besides `@AGENTS.md` into AGENTS.md, then keep only `@AGENTS.md` in CLAUDE.md",
    ]) => cases::claude_extra_line,
    /// A repo without `AGENTS.md` fails at line 1, the whole-file line.
    missing_agents: FailsWith(&[
        "AGENTS.md:1: AGENTS.md does not exist\n    \
         fix: create AGENTS.md at the repo root, or restore it from git history",
    ]) => cases::missing_agents,
    /// A Markdown file with the links, paths and commands it must have passes.
    clean_docs_pass: Passes => cases::clean_docs_pass,
    /// `AGENTS.md` at exactly 120 lines and 8192 bytes passes.
    agents_at_120_lines_and_8192_bytes_passes: Passes => cases::agents_at_120_lines_and_8192_bytes_passes,
    /// `CLAUDE.md` without a final newline is still exactly `@AGENTS.md`.
    claude_without_newline_passes: Passes => cases::claude_without_newline_passes,
    /// Escaped brackets are text, not links, and a fence hides its contents.
    escaped_brackets_and_fences_pass: Passes => cases::escaped_brackets_and_fences_pass,
}
