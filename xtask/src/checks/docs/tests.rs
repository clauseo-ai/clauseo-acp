//! Tests of the docs check's file list, and of the check as one run. The planted cases are in
//! `tests/planted.rs` and their builders in `tests/repos.rs`; the functions below are the expected
//! lines that several cases share.

pub(super) mod planted;
mod repos;

use super::*;
use crate::checks::planted::{fails, failure};
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;

#[test]
fn only_the_markdown_files_of_a_git_repo_are_counted() {
    let dir = Scratch::new("docs-git-count");
    let git = Cmd::new(Program::Git, ["init", "-q"], crate::process::TEST_TIMEOUT);
    let quiet = process::run_quiet(&git, &dir).expect("git runs");
    assert!(quiet.success, "git init failed: {}", quiet.stderr);
    Scratch::write(dir.join("NOTES.md"), "# Notes\n").expect("write a doc");
    Scratch::write(dir.join("data.txt"), "not Markdown\n").expect("write a text file");
    let found = run(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    });
    assert_eq!(found.inspected, 1);
}

#[test]
fn a_folder_that_is_not_a_git_repo_inspects_nothing_and_says_git_s_error() {
    let dir = Scratch::new("docs-no-git");
    Scratch::write(dir.join("NOTES.md"), "# Notes\n").expect("write a doc");
    let found = run(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    });
    let error = crate::changes::repo_files(&dir).expect_err("not a git repo");
    assert_eq!(found, checks::unlisted("docs", &error));
}

#[test]
fn violations_are_listed_by_file_then_line() {
    let dir = Scratch::new("docs-order");
    Scratch::write(dir.join("AGENTS.md"), "# Agents\n").expect("write AGENTS.md");
    Scratch::write(dir.join("ARCHITECTURE.md"), "# Architecture\n").expect("write ARCHITECTURE.md");
    Scratch::write(dir.join("CLAUDE.md"), "@AGENTS.md\n").expect("write CLAUDE.md");
    Scratch::write(dir.join("b.md"), "# B\n\nSee [x](gone.md).\n").expect("write b.md");
    Scratch::write(
        dir.join("a.md"),
        "# A\n\nSee `crates/x/`.\n\n[y](gone.md)\n",
    )
    .expect("write a.md");
    let found = inspect(&dir, &["b.md".to_owned(), "a.md".to_owned()]);
    let places: Vec<String> = found
        .violations
        .iter()
        .map(|violation| format!("{}:{}", violation.path, violation.line))
        .collect();
    assert_eq!(places, ["a.md:3", "a.md:5", "b.md:3"]);
}

#[test]
fn only_the_markdown_files_it_read_are_counted() {
    let dir = Scratch::new("docs-count-read");
    Scratch::write(dir.join("a.md"), "# A\n").expect("write a doc");
    Scratch::write(dir.join("bad.md"), [0xff, 0xfe, b'\n']).expect("write a doc that is not UTF-8");
    let listed = ["a.md", "bad.md", "gone.md"].map(str::to_owned);
    let found = inspect(&dir, &listed);
    assert_eq!(
        found.inspected, 1,
        "a.md is read; bad.md is unreadable, gone.md is not there"
    );
    assert!(
        found
            .violations
            .iter()
            .any(|violation| violation.path == "bad.md"),
        "the unreadable file is reported: {:?}",
        found.violations
    );
}

#[test]
fn a_link_to_a_file_git_ignores_fails_in_a_real_repo() {
    let dir = Scratch::new("docs-git-ignored");
    let git = Cmd::new(Program::Git, ["init", "-q"], crate::process::TEST_TIMEOUT);
    let quiet = process::run_quiet(&git, &dir).expect("git runs");
    assert!(quiet.success, "git init failed: {}", quiet.stderr);
    Scratch::write(dir.join(".gitignore"), "local.md\n").expect("write .gitignore");
    Scratch::write(dir.join("local.md"), "# Local\n").expect("write the ignored file");
    Scratch::write(dir.join("GUIDE.md"), "See [x](local.md).\n").expect("write a doc");
    let found = run(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    });
    let printed: Vec<String> = found.violations.iter().map(ToString::to_string).collect();
    assert!(
        printed.contains(
            &"GUIDE.md:1: the relative link `local.md` does not resolve: `local.md` is on disk, \
              but git ignores it\n    fix: link a file git tracks, or remove the link"
                .to_owned()
        ),
        "the ignored file does not count, and the fix says git ignores it: {printed:?}"
    );
}

/// The violation for the relative link `link` at `file:line`, which names a file that does not
/// exist.
fn bad_link(file: &str, line: usize, link: &str) -> String {
    fails(
        &format!("{file}:{line}"),
        &format!("the relative link `{link}` does not resolve: `{link}` does not exist"),
        Fix::CreateOrCorrectLink {
            path: link.to_owned(),
        },
    )
}

/// The violation for the backticked path `path` at `GUIDE.md:line`, which does not exist.
fn bad_path(line: usize, path: &str) -> String {
    fails(
        &format!("GUIDE.md:{line}"),
        &format!("`{path}` does not exist"),
        Fix::CreateOrCorrectReference {
            path: path.to_owned(),
        },
    )
}

/// The violation for the xtask command `command` at `GUIDE.md:line`, which does not parse for the
/// reason `detail`.
fn bad_command(line: usize, command: &str, detail: &str) -> String {
    fails(
        &format!("GUIDE.md:{line}"),
        &format!("the xtask command `{command}` does not parse: {detail}"),
        Fix::CorrectCommand,
    )
}

/// The zero-inspected failure of the docs check, as it prints when no Markdown file is read.
fn no_markdown_inspected() -> String {
    failure(
        "0 Markdown files inspected in the repo's git-tracked or unignored .md and .markdown files",
        crate::checks::Fix::NothingInspected {
            name: "docs",
            many: "Markdown files",
            place: "in the repo's git-tracked or unignored .md and .markdown files".to_owned(),
            source: "xtask/src/checks/docs.rs",
        },
    )
}
