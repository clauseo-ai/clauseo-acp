//! Tests of the docs check's file list, and of the check as one run.

use super::*;
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;
use std::fs;

#[test]
fn only_the_markdown_files_of_a_git_repo_are_counted() {
    let dir = Scratch::new("docs-git-count");
    let git = Cmd::new(Program::Named("git"), ["init", "-q"]);
    let quiet = process::run_quiet(&git, &dir).expect("git runs");
    assert!(quiet.success, "git init failed: {}", quiet.stderr);
    fs::write(dir.join("NOTES.md"), "# Notes\n").expect("write a doc");
    fs::write(dir.join("data.txt"), "not Markdown\n").expect("write a text file");
    let found = run(&Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
    });
    assert_eq!(found.inspected, 1);
}

#[test]
fn a_folder_that_is_not_a_git_repo_inspects_nothing_and_reports_no_violation() {
    let dir = Scratch::new("docs-no-git");
    fs::write(dir.join("NOTES.md"), "# Notes\n").expect("write a doc");
    let found = run(&Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
    });
    assert_eq!(found, Inspection::default());
}

#[test]
fn violations_are_listed_by_file_then_line() {
    let dir = Scratch::new("docs-order");
    fs::write(dir.join("AGENTS.md"), "# Agents\n").expect("write AGENTS.md");
    fs::write(dir.join("ARCHITECTURE.md"), "# Architecture\n").expect("write ARCHITECTURE.md");
    fs::write(dir.join("CLAUDE.md"), "@AGENTS.md\n").expect("write CLAUDE.md");
    fs::write(dir.join("b.md"), "# B\n\nSee [x](gone.md).\n").expect("write b.md");
    fs::write(
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
fn the_check_is_registered_under_its_name() {
    assert!(crate::checks::ALL.iter().any(|check| check.name == "docs"));
}
