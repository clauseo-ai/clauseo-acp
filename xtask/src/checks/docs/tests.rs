//! Tests of the docs check's file list, and of the check as one run.

mod cases;
pub(super) mod planted;

use super::*;
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;

#[test]
fn only_the_markdown_files_of_a_git_repo_are_counted() {
    let dir = Scratch::new("docs-git-count");
    let git = Cmd::new(Program::Git, ["init", "-q"]);
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
    let git = Cmd::new(Program::Git, ["init", "-q"]);
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
