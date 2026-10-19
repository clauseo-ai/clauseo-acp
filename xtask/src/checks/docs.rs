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
    place: || {
        format!(
            "in the repo's git-tracked or unignored .{} files",
            EXTENSIONS.join(" and .")
        )
    },
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// The extensions of a Markdown file, matched in any case.
const EXTENSIONS: [&str; 2] = ["md", "markdown"];

fn run(ctx: &Context) -> Inspection {
    match changes::repo_files(&ctx.root) {
        Ok(files) => inspect(&ctx.root, &files),
        Err(error) => checks::unlisted(CHECK.name, &error),
    }
}

/// Checks the Markdown files among `files` (repo-relative) under `root`, and the required files.
/// Counts each Markdown file it could read.
fn inspect(root: &Path, files: &[String]) -> Inspection {
    let planned = planned_rows(root);
    let mut inspected = 0;
    let mut violations = Vec::new();
    for file in files.iter().filter(|file| is_markdown(file)) {
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

/// True for a Markdown file by its extension, in any case: `.md` or `.markdown`.
fn is_markdown(file: &str) -> bool {
    Path::new(file)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
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
