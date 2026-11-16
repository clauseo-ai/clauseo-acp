//! Unit tests of the unix-paths check. Its planted cases are in `tests/planted.rs`, and the repos
//! they run on are built in `tests/repos.rs`; the lines below are the unit tests and the line
//! several cases expect.

pub(super) mod planted;
mod repos;

use super::*;
use crate::checks::planted::fails;

/// The failure for the flagged literal `value` at `at`, with no baseline line, as it prints.
fn flagged(at: &str, value: &str) -> String {
    fails(
        at,
        &format!("{value:?} is a Unix absolute path, which is not absolute on Windows"),
        Fix::UseAbs,
    )
}

#[test]
fn files_in_scope_are_tests_under_tests_or_named_or_foldered_tests_under_src() {
    let cases = [
        ("crates/host/tests/a.rs", Scope::Test),
        ("crates/host/tests/a/b.rs", Scope::Test),
        ("crates/host/src/import_tests.rs", Scope::Test),
        ("crates/host/src/tests.rs", Scope::Test),
        ("crates/host/src/agent/tests/redact.rs", Scope::Test),
        ("crates/host/src/agent/redact.rs", Scope::Source),
        ("crates/host/src/lib.rs", Scope::Source),
        ("crates/host/benches/a.rs", Scope::Source),
        ("crates/host/src/testing.rs", Scope::Data),
        ("crates/protocol/tests/corpus/writer.rs", Scope::Data),
        ("crates/protocol/tests/snapshots/mcp.rs", Scope::Data),
    ];
    for (file, expected) in cases {
        assert_eq!(scope(file), expected, "{file}");
    }
}

#[test]
fn a_baseline_line_is_a_path_and_a_quoted_literal_with_escapes() {
    let (entries, violations) =
        parse("# header\n\na/b.rs \"/opt/x y\"\na/c.rs \"/opt/\\\"q\\\"\"\n");
    assert_eq!(violations, []);
    let keys: Vec<(&str, &str, u32)> = entries
        .iter()
        .map(|entry| (entry.key.0.as_str(), entry.key.1.as_str(), entry.line))
        .collect();
    assert_eq!(
        keys,
        [("a/b.rs", "/opt/x y", 3), ("a/c.rs", "/opt/\"q\"", 4)]
    );
}

#[test]
fn the_real_repo_has_no_file_that_does_not_lex_and_counts_its_baseline() {
    let root = crate::workspace::workspace_root();
    let ctx = Context {
        root,
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    };
    let found = run(&ctx);
    assert_eq!(found.violations, []);
    assert!(found.inspected > 0);
    assert!(
        found.notes[0].starts_with("unix-paths: "),
        "{:?}",
        found.notes
    );
}
