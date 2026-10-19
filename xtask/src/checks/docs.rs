//! The `docs` check: the Markdown files of the repo, read with `pulldown-cmark`. In every file,
//! relative links resolve, backticked repo paths exist and `cargo xtask` commands parse against
//! the clap tree. The files in `required::REQUIRED` also keep their limits and their content. It
//! counts the Markdown files it read.
//!
//! The files come from `changes::repo_files`: tracked files and also untracked files that are not
//! ignored (`git ls-files --others`). That is on purpose, not a slip: it catches a new file before
//! anyone adds it, and it matches what `--quick` sees. Links and paths are looked up in the same
//! list, so an ignored local file does not count as existing; the disk is read only to say so.

mod commands;
mod links;
mod markdown;
mod paths;
mod required;
mod tree;

#[cfg(test)]
mod tests;

use crate::changes;
use crate::checks::crates_table::{self, Status};
use crate::checks::{self, Inspection, Noun, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::path::Path;
use tree::Tree;

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
/// Links and backticked paths must name one of `files` or a folder above one. Counts each
/// Markdown file it read.
fn inspect(root: &Path, files: &[String]) -> Inspection {
    let planned = planned_rows(root);
    let tree = Tree::new(files);
    let mut inspected = 0;
    let mut violations = Vec::new();
    for file in files.iter().filter(|file| is_markdown(file)) {
        match checks::read_repo_file(root, file) {
            Ok(Some(text)) => {
                inspected += 1;
                violations.extend(file_problems(&tree, root, file, &text, &planned));
            }
            Ok(None) => {}
            Err(unreadable) => violations.push(unreadable),
        }
    }
    violations.extend(required_problems(root));
    // Sorted by the whole violation, so `dedup` merges every repeat, not only adjacent ones.
    violations.sort_by(|a, b| {
        (a.path.as_str(), a.line, a.message.as_str(), a.fix.as_str()).cmp(&(
            b.path.as_str(),
            b.line,
            b.message.as_str(),
            b.fix.as_str(),
        ))
    });
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

/// Every problem in one Markdown file under `root`: its links, its repo paths and its xtask
/// commands.
fn file_problems(
    tree: &Tree,
    root: &Path,
    file: &str,
    text: &str,
    planned: &[String],
) -> Vec<Violation> {
    let scan = markdown::scan(text);
    let mut found = links::problems(tree, root, file, &scan.links);
    found.extend(paths::problems(tree, root, file, &scan.spans, planned));
    found.extend(commands::problems(file, &scan));
    found
}

/// The problems with the required files: missing, over a limit, or not as they must read.
fn required_problems(root: &Path) -> Vec<Violation> {
    required::REQUIRED
        .iter()
        .flat_map(
            |required| match checks::read_repo_file(root, required.file) {
                Ok(Some(text)) => required::problems(required, &text),
                Ok(None) => vec![required::missing(required)],
                Err(unreadable) => vec![unreadable],
            },
        )
        .collect()
}
