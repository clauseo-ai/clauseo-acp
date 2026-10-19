//! Tests of the limits and the import rule of the required files.

use super::*;
use crate::checks::file_size;
use std::fs;
use std::path::Path;

const AGENTS: &str = "AGENTS.md";
const ARCHITECTURE: &str = "ARCHITECTURE.md";
const CLAUDE: &str = "CLAUDE.md";

/// The row of `REQUIRED` for `file`.
fn row(file: &str) -> Required {
    REQUIRED
        .into_iter()
        .find(|required| required.file == file)
        .unwrap_or_else(|| panic!("{file} is a required file"))
}

/// The line and byte limits of `file`.
fn limits(file: &str) -> (usize, Option<usize>) {
    match row(file).rule {
        Rule::Limits { lines, bytes } => (lines, bytes),
        Rule::Import(_) => panic!("{file} has limits"),
    }
}

/// The messages of `problems` for `file`, one per line.
fn messages(file: &str, text: &str) -> Vec<String> {
    problems(&row(file), text)
        .into_iter()
        .map(|found| format!("{}:{}: {}", found.path, found.line, found.message))
        .collect()
}

#[test]
fn a_file_at_its_line_limit_passes_and_one_over_fails_at_the_first_line_over() {
    let at = "x\n".repeat(120);
    assert_eq!(messages(AGENTS, &at), Vec::<String>::new());
    let over = "x\n".repeat(121);
    assert_eq!(
        messages(AGENTS, &over),
        ["AGENTS.md:121: AGENTS.md has 121 lines, over the limit of 120"]
    );
}

#[test]
fn the_byte_limit_names_the_line_of_the_first_byte_over_it() {
    let first_line_is_over = format!("{}\nb\n", "a".repeat(8193));
    assert_eq!(
        messages(AGENTS, &first_line_is_over),
        ["AGENTS.md:1: AGENTS.md is 8196 bytes, over the limit of 8192"]
    );
    let second_line_is_over = format!("{}\n{}\n", "a".repeat(8190), "b".repeat(5));
    assert_eq!(
        messages(AGENTS, &second_line_is_over),
        ["AGENTS.md:2: AGENTS.md is 8197 bytes, over the limit of 8192"]
    );
}

#[test]
fn architecture_has_its_own_line_limit() {
    assert_eq!(
        messages(ARCHITECTURE, &"x\n".repeat(300)),
        Vec::<String>::new()
    );
    assert_eq!(
        messages(ARCHITECTURE, &"x\n".repeat(301)),
        ["ARCHITECTURE.md:301: ARCHITECTURE.md has 301 lines, over the limit of 300"]
    );
}

#[test]
fn claude_is_the_import_with_or_without_its_final_newline() {
    assert_eq!(messages(CLAUDE, "@AGENTS.md\n"), Vec::<String>::new());
    assert_eq!(messages(CLAUDE, "@AGENTS.md"), Vec::<String>::new());
}

#[test]
fn claude_points_at_line_2_only_when_text_follows_the_import() {
    assert_eq!(
        messages(CLAUDE, "@AGENTS.md\n\n"),
        ["CLAUDE.md:2: CLAUDE.md must hold only `@AGENTS.md`"]
    );
    assert_eq!(
        messages(CLAUDE, "@AGENTS.md\r\n"),
        ["CLAUDE.md:1: CLAUDE.md must hold only `@AGENTS.md`"]
    );
    assert_eq!(
        messages(CLAUDE, "# Notes\n@AGENTS.md\n"),
        ["CLAUDE.md:1: CLAUDE.md must hold only `@AGENTS.md`"]
    );
    assert_eq!(
        messages(CLAUDE, ""),
        ["CLAUDE.md:1: CLAUDE.md must hold only `@AGENTS.md`"]
    );
}

#[test]
fn a_missing_required_file_is_at_line_1_with_a_fix_that_creates_it() {
    let found = missing(&row(CLAUDE));
    assert_eq!(found.line, 1);
    assert_eq!(found.message, "CLAUDE.md does not exist");
    assert_eq!(
        found.fix,
        "create CLAUDE.md holding only `@AGENTS.md`, or restore it from git history"
    );
    assert_eq!(
        missing(&row(ARCHITECTURE)).fix,
        "create ARCHITECTURE.md at the repo root, or restore it from git history"
    );
}

/// The real `file` at the repo root, its line breaks read as spaces so a wrapped sentence is one.
fn real_doc(file: &str) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let text = fs::read_to_string(root.join(file)).unwrap_or_else(|e| panic!("read {file}: {e}"));
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Fails unless `file` says `sentence`, the limit as the check enforces it.
fn assert_says(file: &str, sentence: &str) {
    assert!(
        real_doc(file).contains(sentence),
        "{file} must state the limit the check enforces, as: {sentence}"
    );
}

#[test]
fn the_docs_state_the_limits_the_checks_enforce() {
    let (agents_lines, agents_bytes) = limits(AGENTS);
    let kib = agents_bytes.expect("AGENTS.md has a byte limit") / 1024;
    assert_says(
        AGENTS,
        &format!("Keep this file to at most {agents_lines} lines and {kib} KiB."),
    );
    let (architecture_lines, _) = limits(ARCHITECTURE);
    assert_says(
        ARCHITECTURE,
        &format!("Keep this file to at most {architecture_lines} lines."),
    );
    let source_lines = file_size::LINE_LIMIT;
    assert_says(
        AGENTS,
        &format!("A source file has at most {source_lines} lines."),
    );
}

#[test]
fn the_byte_limit_is_a_whole_number_of_kib() {
    let (_, bytes) = limits(AGENTS);
    assert_eq!(bytes.map(|bytes| bytes % 1024), Some(0));
}
