//! Planted cases of the file-size check, and tests of its helpers. Each planted case writes its
//! files into a scratch git repo and runs the real `run` on it.

pub(super) mod planted;

use super::*;
use crate::process::{self, Cmd, Program};
use crate::scratch::Scratch;

/// A scratch git repo named after `case`, with each `(path, text)` of `files` written into it.
fn repo(case: &str, files: &[(&str, String)]) -> Scratch {
    let dir = Scratch::new(&format!("planted-file-size-{case}"));
    let init = Cmd::new(Program::Git, ["init", "-q"]);
    let quiet = process::run_quiet(&init, &dir).expect("run git init");
    assert!(quiet.success, "git init failed: {}", quiet.stderr);
    for (path, text) in files {
        let full = dir.join(path);
        Scratch::create_dir_all(full.parent().expect("a file has a parent")).expect("create dirs");
        Scratch::write(full, text).expect("write a file");
    }
    dir
}

/// Runs the check on the scratch repo that `files` make.
fn run_on(case: &str, files: &[(&str, String)]) -> Inspection {
    let dir = repo(case, files);
    run(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    })
}

/// A file of `n` lines.
fn lines(n: usize) -> String {
    "\n".repeat(n)
}

/// The exception file with `entries` after one `#` header line.
fn exceptions(entries: &str) -> (&'static str, String) {
    (EXCEPTIONS, format!("# a header line\n{entries}"))
}

pub(super) fn rs_601_lines() -> Inspection {
    run_on("rs-601-lines", &[("src/big.rs", lines(601))])
}

pub(super) fn ts_601_lines() -> Inspection {
    run_on("ts-601-lines", &[("ui/big.ts", lines(601))])
}

pub(super) fn tsx_601_lines() -> Inspection {
    run_on("tsx-601-lines", &[("ui/Big.tsx", lines(601))])
}

pub(super) fn mts_and_upper_case_rs_601_lines() -> Inspection {
    run_on(
        "mts-and-upper-case-rs-601-lines",
        &[("ui/big.mts", lines(601)), ("src/LIB.RS", lines(601))],
    )
}

pub(super) fn stale_exception_gone() -> Inspection {
    run_on(
        "stale-exception-gone",
        &[
            ("src/lib.rs", "fn f() {}\n".to_owned()),
            exceptions("src/gone.rs old generator\n"),
        ],
    )
}

pub(super) fn stale_exception_short() -> Inspection {
    run_on(
        "stale-exception-short",
        &[
            ("src/short.rs", lines(450)),
            exceptions("src/short.rs was once long\n"),
        ],
    )
}

pub(super) fn line_comment_101() -> Inspection {
    let text = format!("fn a() {{}}\n// {}\n", "a".repeat(98));
    run_on("line-comment-101", &[("src/lib.rs", text)])
}

pub(super) fn doc_comment_101() -> Inspection {
    let text = format!("//! {}\n", "a".repeat(97));
    run_on("doc-comment-101", &[("src/lib.rs", text)])
}

pub(super) fn trailing_comment_past_100() -> Inspection {
    // The code is 14 characters and 82 spaces; the comment `// end` is 6 more: 102 in all.
    let text = format!("let value = 1;{}// end\n", " ".repeat(82));
    run_on("trailing-comment-past-100", &[("src/lib.rs", text)])
}

pub(super) fn block_comment_past_100() -> Inspection {
    // Line 2 is three spaces, 104 `y`, and ` */`: 110 characters.
    let text = format!("/* first\n   {} */\nfn f() {{}}\n", "y".repeat(104));
    run_on("block-comment-past-100", &[("src/lib.rs", text)])
}

pub(super) fn e_acute_comment_100_passes() -> Inspection {
    let text = format!("//{}\n", "é".repeat(98));
    run_on("e-acute-comment-100", &[("src/lib.rs", text)])
}

pub(super) fn string_literal_101_passes() -> Inspection {
    // The literal is 101 characters with its quotes; the line is over 100 because of it.
    let text = format!("let s = \"{}\";\n", "a".repeat(99));
    run_on("string-literal-101", &[("src/lib.rs", text)])
}

pub(super) fn string_holding_slashes_101_passes() -> Inspection {
    let text = format!("let url = \"https://{}\";\n", "a".repeat(84));
    run_on("string-holding-slashes-101", &[("src/lib.rs", text)])
}

pub(super) fn raw_string_holding_slashes_passes() -> Inspection {
    let text = format!(
        "let s = r#\"https://example.com\n// {}\n\"#;\nlet t = r#\"// a\"#;\n",
        "b".repeat(120)
    );
    run_on("raw-string-holding-slashes", &[("src/lib.rs", text)])
}

pub(super) fn char_quote_then_code_passes() -> Inspection {
    let text = format!(
        "let quote = '\"';\nlet url = \"https://example.com/{}\";\n",
        "c".repeat(60)
    );
    run_on("char-quote-then-code", &[("src/lib.rs", text)])
}

pub(super) fn lifetime_then_string_passes() -> Inspection {
    let text = format!(
        "fn pick<'a>(first: &'a str) -> &'static str {{\n    'outer: loop {{ break 'outer; }}\n    \
         let url = \"https://example.com/{}\";\n    first\n}}\n",
        "d".repeat(60)
    );
    run_on("lifetime-then-string", &[("src/lib.rs", text)])
}

pub(super) fn listed_600_plus_file_passes() -> Inspection {
    run_on(
        "listed-600-plus",
        &[
            ("src/parser.rs", lines(650)),
            exceptions("src/parser.rs generated by a grammar tool\n"),
        ],
    )
}

#[test]
fn a_file_of_exactly_the_limit_passes() {
    let found = run_on("exactly-600", &[("src/lib.rs", lines(LINE_LIMIT))]);
    assert_eq!((found.inspected, found.violations), (1, Vec::new()));
}

#[test]
fn only_rs_ts_and_tsx_files_are_inspected_and_only_rs_comments_are_checked() {
    let long_comment = format!("// {}\n", "a".repeat(150));
    let found = run_on(
        "kinds",
        &[
            ("README.md", lines(900)),
            ("ui/notes.ts", long_comment),
            ("src/lib.rs", "fn f() {}\n".to_owned()),
        ],
    );
    assert_eq!((found.inspected, found.violations), (2, Vec::new()));
}

#[test]
fn a_listed_file_over_the_limit_still_has_its_comments_checked() {
    let long_comment = format!("// {}\n", "a".repeat(99));
    let found = run_on(
        "listed-comment",
        &[
            ("src/parser.rs", format!("{long_comment}{}", lines(650))),
            exceptions("src/parser.rs generated\n"),
        ],
    );
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
    let dir = repo("unreadable-listed", &[("src/bad.rs", String::new())]);
    Scratch::write(dir.join("src/bad.rs"), [0xff, 0xfe, b'\n']).expect("write bytes");
    Scratch::create_dir_all(dir.join("xtask")).expect("create xtask");
    Scratch::write(dir.join(EXCEPTIONS), "src/bad.rs not UTF-8\n").expect("write exceptions");
    let found = run(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    });
    let paths: Vec<&str> = found.violations.iter().map(|v| v.path.as_str()).collect();
    assert_eq!(paths, ["src/bad.rs"], "{:?}", found.violations);
}

pub(super) fn not_a_git_repo() -> Inspection {
    let dir = Scratch::new("planted-file-size-not-a-git-repo");
    Scratch::write(dir.join("lib.rs"), "fn f() {}\n").expect("write a source");
    run(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    })
}

pub(super) fn only_file_unreadable() -> Inspection {
    let dir = repo("only-file-unreadable", &[]);
    Scratch::write(dir.join("bad.rs"), [0xff, 0xfe, b'\n']).expect("write bytes");
    run(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    })
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
