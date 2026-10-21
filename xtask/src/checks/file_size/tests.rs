//! Unit tests of the file-size check. Its planted cases are in `tests/planted.rs`, and the repos
//! they run on are built in `tests/repos.rs`; the lines below are the unit tests and the
//! helpers of those tests.

pub(super) mod planted;
mod repos;

use super::*;
use crate::scratch::Scratch;
use repos::{exceptions, git_repo, lines};

#[test]
fn a_file_of_exactly_the_limit_passes() {
    let found = run(git_repo("exactly-600", &[("src/lib.rs", lines(LINE_LIMIT))]).ctx());
    assert_eq!((found.inspected, found.violations), (1, Vec::new()));
}

#[test]
fn only_rs_ts_and_tsx_files_are_inspected_and_only_rs_comments_are_checked() {
    let long_comment = format!("// {}\n", "a".repeat(150));
    let found = run(git_repo(
        "kinds",
        &[
            ("README.md", lines(900)),
            ("ui/notes.ts", long_comment),
            ("src/lib.rs", "fn f() {}\n".to_owned()),
        ],
    )
    .ctx());
    assert_eq!((found.inspected, found.violations), (2, Vec::new()));
}

#[test]
fn a_listed_file_over_the_limit_still_has_its_comments_checked() {
    let long_comment = format!("// {}\n", "a".repeat(99));
    let found = run(git_repo(
        "listed-comment",
        &[
            ("src/parser.rs", format!("{long_comment}{}", lines(650))),
            exceptions("src/parser.rs generated\n"),
        ],
    )
    .ctx());
    let printed: Vec<String> = found.violations.iter().map(ToString::to_string).collect();
    assert_eq!(
        printed,
        [
            "src/parser.rs:1: this comment reaches column 102; the limit is 100\n    \
          fix: wrap the comment at 100 columns"
        ]
    );
}

#[test]
fn an_unreadable_listed_file_is_not_called_stale() {
    let repo = git_repo(
        "unreadable-listed",
        &[
            ("src/bad.rs", [0xff, 0xfe, b'\n'].as_slice()),
            (EXCEPTIONS, b"src/bad.rs not UTF-8\n".as_slice()),
        ],
    );
    let found = run(repo.ctx());
    let paths: Vec<&str> = found.violations.iter().map(|v| v.path.as_str()).collect();
    assert_eq!(paths, ["src/bad.rs"], "{:?}", found.violations);
}

#[test]
fn lines_end_at_lf_crlf_or_a_lone_cr() {
    assert_eq!(checks::line_count(b""), 0);
    assert_eq!(checks::line_count(b"a"), 1);
    assert_eq!(checks::line_count(b"a\n"), 1);
    assert_eq!(checks::line_count(b"a\nb"), 2);
    assert_eq!(checks::line_count(b"a\r\nb\r\n"), 2);
    assert_eq!(checks::line_count(b"a\rb\rc"), 3);
    assert_eq!(checks::line_count(b"\n\r\r\n"), 3);
}

#[test]
fn near_limit_notes_name_changed_files_of_501_to_600_lines() {
    let sizes = [
        ("a.rs".to_owned(), 500),
        ("b.rs".to_owned(), 501),
        ("c.ts".to_owned(), 600),
        ("d.rs".to_owned(), 601),
        ("e.rs".to_owned(), 550),
    ];
    let changed = ["a.rs", "b.rs", "c.ts", "d.rs", "gone.rs"].map(str::to_owned);
    assert_eq!(
        near_limit_notes(&changed, &sizes),
        [
            "b.rs has 501 of 600 lines; split it now",
            "c.ts has 600 of 600 lines; split it now",
        ]
    );
}

#[test]
fn a_changed_file_list_that_fails_gives_one_note_saying_so() {
    let dir = Scratch::new("file-size-notes-no-base");
    let found = notes(&dir, &[("a.rs".to_owned(), 550)]);
    assert_eq!(found.len(), 1);
    assert!(
        found[0].starts_with("no near-limit notes, since the changed files are unknown: "),
        "{found:?}"
    );
}
