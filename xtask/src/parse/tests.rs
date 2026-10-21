//! Tests of the parsers in `parse.rs`: rustfmt's and clippy's output, the findings they become,
//! and the help notes clippy attaches to a finding. Clippy's JSON is copied from real runs (see
//! the note above each fixture).

use super::*;

#[test]
fn clippy_repro_drops_the_workspace_flag_and_narrows_to_the_owner() {
    let cmd = Cmd::new(
        crate::process::Program::Cargo,
        [
            "clippy",
            "--workspace",
            "--all-targets",
            "--message-format=short",
            "--",
            "-D",
            "warnings",
        ],
        crate::process::TEST_TIMEOUT,
    );
    let members = vec![Member {
        name: "xtask".into(),
        dir: "xtask".into(),
        path_deps: vec![],
        targets: vec![],
        dependencies: crate::workspace::PackageDependencies::default(),
    }];
    assert_eq!(
        clippy_repro(&cmd, "xtask/src/main.rs", &members).to_string(),
        "cargo clippy -p xtask --all-targets -- -D warnings"
    );
}

#[test]
fn clippy_repro_copies_everything_after_the_separator_verbatim() {
    let cmd = Cmd::new(
        crate::process::Program::Cargo,
        [
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-p",
            "odd",
            "--workspace",
            "--message-format=short",
        ],
        crate::process::TEST_TIMEOUT,
    );
    let members = vec![Member {
        name: "xtask".into(),
        dir: "xtask".into(),
        path_deps: vec![],
        targets: vec![],
        dependencies: crate::workspace::PackageDependencies::default(),
    }];
    assert_eq!(
        clippy_repro(&cmd, "xtask/src/main.rs", &members).to_string(),
        "cargo clippy -p xtask --all-targets -- -p odd --workspace --message-format=short"
    );
}

#[test]
fn rel_path_strips_root_and_normalises_separators() {
    assert_eq!(
        rel_path("/ws/xtask/src/main.rs", "/ws"),
        "xtask/src/main.rs"
    );
    assert_eq!(rel_path("./xtask/src/main.rs", "/ws"), "xtask/src/main.rs");
    assert_eq!(
        rel_path("C:\\ws\\xtask\\src\\main.rs", "C:\\ws"),
        "xtask/src/main.rs"
    );
}

#[test]
fn fmt_parses_current_header() {
    let out = "Diff in /ws/xtask/src/main.rs:3:\n-fn  bad( ){}\n+fn bad() {}\n";
    assert_eq!(
        fmt_diffs(out, "/ws"),
        vec![("xtask/src/main.rs".to_owned(), 3)]
    );
}

#[test]
fn fmt_reports_the_changed_line_not_the_hunk_context_start() {
    // Real `cargo fmt --check` output for `fn  bad( ){}` appended at line 40.
    let out = "Diff in /ws/xtask/src/main.rs:37:\n         Commands::Setup => tools::setup(),\n     }\n }\n-fn  bad( ){}\n+fn bad() {}\n\n";
    assert_eq!(
        fmt_diffs(out, "/ws"),
        vec![("xtask/src/main.rs".to_owned(), 40)]
    );
}

#[test]
fn fmt_points_at_the_inserted_line_for_an_add_only_hunk() {
    // Three context lines (37-39), then an added line that goes in front of original line 40.
    let out = "Diff in /ws/xtask/src/main.rs:37:\n         Commands::Setup => setup::run(),\n     }\n }\n+fn added() {}\n\n";
    assert_eq!(
        fmt_diffs(out, "/ws"),
        vec![("xtask/src/main.rs".to_owned(), 40)]
    );
}

#[test]
fn fmt_parses_old_header_and_dedupes() {
    let out = "Diff in /ws/xtask/src/main.rs at line 3:\n\
               Diff in /ws/xtask/src/main.rs at line 3:\n\
               Diff in /ws/xtask/src/lib.rs at line 9:\n";
    assert_eq!(
        fmt_diffs(out, "/ws"),
        vec![
            ("xtask/src/main.rs".to_owned(), 3),
            ("xtask/src/lib.rs".to_owned(), 9)
        ]
    );
}

#[test]
fn fmt_parses_windows_path() {
    let out = "Diff in C:\\ws\\xtask\\src\\main.rs:7:\n";
    assert_eq!(
        fmt_diffs(out, "C:\\ws"),
        vec![("xtask/src/main.rs".to_owned(), 7)]
    );
}

#[test]
fn fmt_finds_nothing_in_a_syntax_error() {
    let out = "error: expected one of `!` or `::`, found `(`\n --> xtask/src/main.rs:1:4\n";
    assert_eq!(fmt_diffs(out, "/ws"), Vec::<(String, u32)>::new());
}

#[test]
fn fmt_findings_make_one_error_per_file_fmt_would_change_with_the_fmt_fix() {
    let ctx = Context {
        root: std::path::PathBuf::from("/ws"),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    };
    let cmd = Cmd::new(
        crate::process::Program::Cargo,
        ["fmt", "--all", "--", "--check"],
        crate::process::TEST_TIMEOUT,
    );
    let out = "Diff in /ws/xtask/src/main.rs:3:\n-fn  bad( ){}\n+fn bad() {}\n";
    let findings = fmt_findings(out, &ctx, &cmd);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].is_error);
    assert_eq!(
        findings[0].violation.to_string(),
        "xtask/src/main.rs:3: rustfmt would reformat this code\n    fix: run `cargo fmt --all`"
    );
}

#[test]
fn clippy_parses_warning_and_error_lines() {
    let out = "xtask/src/main.rs:4:9: warning: unused variable: `x`\n\
               xtask/src/main.rs:6:5: error[E0425]: cannot find value `y` in this scope\n";
    let diags = short_diags(out, "/ws");
    assert_eq!(
        diags,
        vec![
            Diag {
                path: "xtask/src/main.rs".into(),
                line: 4,
                message: "warning: unused variable: `x`".into(),
                is_error: false,
            },
            Diag {
                path: "xtask/src/main.rs".into(),
                line: 6,
                message: "error[E0425]: cannot find value `y` in this scope".into(),
                is_error: true,
            },
        ]
    );
}

#[test]
fn clippy_parses_windows_drive_paths() {
    let out = "C:\\ws\\xtask\\src\\main.rs:4:9: error: unused variable: `unused`\n";
    let diags = short_diags(out, "C:\\ws");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].path, "xtask/src/main.rs");
    assert_eq!(diags[0].line, 4);
    assert!(diags[0].is_error);
}

#[test]
fn clippy_ignores_lines_without_a_file_prefix_and_dedupes() {
    let out = "warning: `xtask` (bin \"xtask\") generated 1 warning\n\
               error: could not compile `xtask` (bin \"xtask\") due to 1 previous error\n\
               xtask/src/main.rs:4:9: warning: unused variable: `x`\n\
               xtask/src/main.rs:4:9: warning: unused variable: `x`\n";
    let diags = short_diags(out, "/ws");
    assert_eq!(diags.len(), 1);
    assert!(!diags[0].is_error);
}

const JSON_OUTPUT: &str = concat!(
    r#"{"reason":"compiler-artifact","target":{"name":"serde"}}"#,
    "\n",
    r#"{"reason":"compiler-message","message":{"rendered":"/ws/xtask/src/check.rs:13:14: error: used `unwrap()` on an `Option` value\n","code":{"code":"clippy::unwrap_used","explanation":null},"level":"error"}}"#,
    "\n",
    r#"{"reason":"compiler-message","message":{"rendered":"xtask/src/main.rs:6:5: error[E0425]: cannot find value `y` in this scope\n","code":{"code":"E0425"},"level":"error"}}"#,
    "\n",
    r#"{"reason":"compiler-message","message":{"rendered":"warning: 1 warning emitted\n","code":null,"level":"warning"}}"#,
    "\n",
    "error: could not compile `xtask` (bin \"xtask\") due to 2 previous errors\n",
);

#[test]
fn clippy_json_names_the_lint_and_does_not_repeat_an_error_code() {
    let diags = short_diags(JSON_OUTPUT, "/ws");
    assert_eq!(
        diags,
        vec![
            Diag {
                path: "xtask/src/check.rs".into(),
                line: 13,
                message: "error: used `unwrap()` on an `Option` value (clippy::unwrap_used)".into(),
                is_error: true,
            },
            Diag {
                path: "xtask/src/main.rs".into(),
                line: 6,
                message: "error[E0425]: cannot find value `y` in this scope".into(),
                is_error: true,
            },
        ]
    );
}

#[test]
fn clippy_render_keeps_rendered_text_and_plain_lines_and_drops_other_json() {
    assert_eq!(
        clippy_render(JSON_OUTPUT),
        "/ws/xtask/src/check.rs:13:14: error: used `unwrap()` on an `Option` value\n\
         xtask/src/main.rs:6:5: error[E0425]: cannot find value `y` in this scope\n\
         warning: 1 warning emitted\n\
         error: could not compile `xtask` (bin \"xtask\") due to 2 previous errors\n"
    );
}

/// Real clippy output for `let _ = std::fs::canonicalize(".");` in xtask `fn main` (clippy 1.99.0).
/// Cut: `package_id`, `manifest_path` and `target`, which xtask does not read.
const CANONICALIZE: &str = r#"{"reason":"compiler-message","message":{"rendered":"xtask/src/main.rs:113:13: error: use of a disallowed method `std::fs::canonicalize`\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"help","message":"for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#disallowed_methods","rendered":null,"spans":[]},{"children":[],"code":null,"level":"note","message":"requested on the command line with `-D clippy::disallowed-methods`","rendered":null,"spans":[]},{"children":[],"code":null,"level":"help","message":"canonicalize returns \\\\?\\ paths on Windows that most tools refuse","rendered":null,"spans":[{"byte_end":4861,"byte_start":4840,"column_end":34,"column_start":13,"expansion":null,"file_name":"xtask/src/main.rs","is_primary":true,"label":null,"line_end":113,"line_start":113,"suggested_replacement":"dunce::canonicalize","suggestion_applicability":"MachineApplicable","text":[{"highlight_end":34,"highlight_start":13,"text":"    let _ = std::fs::canonicalize(\".\");"}]}]}],"level":"error","message":"use of a disallowed method `std::fs::canonicalize`","spans":[{"byte_end":4861,"byte_start":4840,"column_end":34,"column_start":13,"expansion":null,"file_name":"xtask/src/main.rs","is_primary":true,"label":null,"line_end":113,"line_start":113,"suggested_replacement":null,"suggestion_applicability":null,"text":[{"highlight_end":34,"highlight_start":13,"text":"    let _ = std::fs::canonicalize(\".\");"}]}],"code":{"code":"clippy::disallowed_methods","explanation":null}}}"#;

/// Real clippy output for `let _ = std::fs::write("a", "b");` in the same place. Its reason
/// is a note.
const WRITE: &str = r#"{"reason":"compiler-message","message":{"rendered":"xtask/src/main.rs:114:13: error: use of a disallowed method `std::fs::write`\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"note","message":"files change only through the crate's write module: xtask/src/write.rs in xtask, Scratch (xtask/src/scratch.rs) in xtask's tests; a crate that writes its first file adds its own, with one #[expect] listed in xtask/expect-allowlist.txt","rendered":null,"spans":[]},{"children":[],"code":null,"level":"help","message":"for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#disallowed_methods","rendered":null,"spans":[]}],"level":"error","message":"use of a disallowed method `std::fs::write`","spans":[{"byte_end":4894,"byte_start":4880,"column_end":27,"column_start":13,"expansion":null,"file_name":"xtask/src/main.rs","is_primary":true,"label":null,"line_end":114,"line_start":114,"suggested_replacement":null,"suggestion_applicability":null,"text":[{"highlight_end":27,"highlight_start":13,"text":"    let _ = std::fs::write(\"a\", \"b\");"}]}],"code":{"code":"clippy::disallowed_methods","explanation":null}}}"#;

/// Real clippy output for `let _b = 1 == 1;`: its children are the docs link and the default
/// level only.
const EQ_OP: &str = r#"{"reason":"compiler-message","message":{"rendered":"xtask/src/main.rs:115:14: error: equal expressions as operands to `==`\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"help","message":"for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#eq_op","rendered":null,"spans":[]},{"children":[],"code":null,"level":"note","message":"`#[deny(clippy::eq_op)]` on by default","rendered":null,"spans":[]}],"level":"error","message":"equal expressions as operands to `==`","spans":[{"byte_end":4883,"byte_start":4877,"column_end":20,"column_start":14,"expansion":null,"file_name":"xtask/src/main.rs","is_primary":true,"label":null,"line_end":115,"line_start":115,"suggested_replacement":null,"suggestion_applicability":null,"text":[{"highlight_end":20,"highlight_start":14,"text":"    let _b = 1 == 1;"}]}],"code":{"code":"clippy::eq_op","explanation":null}}}"#;

/// Real clippy output for `let _ = std::fs::canonicalize(".");`, a must-use `Result` bound to `_`:
/// a help without a replacement, and a note that points at the type.
const LET_UNDERSCORE: &str = r#"{"reason":"compiler-message","message":{"rendered":"xtask/src/main.rs:115:5: error: non-binding `let` on an expression with `#[must_use]` type\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"help","message":"consider explicitly using expression value","rendered":null,"spans":[]},{"children":[],"code":null,"level":"note","message":"type is `std::result::Result`","rendered":null,"spans":[{"byte_end":4924,"byte_start":4898,"column_end":39,"column_start":13,"expansion":null,"file_name":"xtask/src/main.rs","is_primary":true,"label":null,"line_end":115,"line_start":115,"suggested_replacement":null,"suggestion_applicability":null,"text":[{"highlight_end":39,"highlight_start":13,"text":"    let _ = std::fs::canonicalize(\".\");"}]}]},{"children":[],"code":null,"level":"help","message":"for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#let_underscore_must_use","rendered":null,"spans":[]},{"children":[],"code":null,"level":"note","message":"requested on the command line with `-D clippy::let-underscore-must-use`","rendered":null,"spans":[]}],"level":"error","message":"non-binding `let` on an expression with `#[must_use]` type","spans":[{"byte_end":4925,"byte_start":4890,"column_end":40,"column_start":5,"expansion":null,"file_name":"xtask/src/main.rs","is_primary":true,"label":null,"line_end":115,"line_start":115,"suggested_replacement":null,"suggestion_applicability":null,"text":[{"highlight_end":40,"highlight_start":5,"text":"    let _ = std::fs::canonicalize(\".\");"}]}],"code":{"code":"clippy::let_underscore_must_use","explanation":null}}}"#;

/// Real clippy output for `let _ = Some(1u8).unwrap();`: its help suggests removing code, so the
/// replacement is empty.
const UNWRAP_SOME: &str = r#"{"reason":"compiler-message","message":{"rendered":"xtask/src/main.rs:114:13: warning: used `unwrap()` on `Some` value\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"help","message":"for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#unnecessary_literal_unwrap","rendered":null,"spans":[]},{"children":[],"code":null,"level":"note","message":"`#[warn(clippy::unnecessary_literal_unwrap)]` on by default","rendered":null,"spans":[]},{"children":[],"code":null,"level":"help","message":"remove the `Some` and `unwrap()`","rendered":null,"spans":[{"byte_end":4871,"byte_start":4866,"column_end":18,"column_start":13,"expansion":null,"file_name":"xtask/src/main.rs","is_primary":true,"label":null,"line_end":114,"line_start":114,"suggested_replacement":"","suggestion_applicability":"MachineApplicable","text":[{"highlight_end":18,"highlight_start":13,"text":"    let _ = Some(1u8).unwrap();"}]},{"byte_end":4884,"byte_start":4874,"column_end":31,"column_start":21,"expansion":null,"file_name":"xtask/src/main.rs","is_primary":true,"label":null,"line_end":114,"line_start":114,"suggested_replacement":"","suggestion_applicability":"MachineApplicable","text":[{"highlight_end":31,"highlight_start":21,"text":"    let _ = Some(1u8).unwrap();"}]}]}],"level":"warning","message":"used `unwrap()` on `Some` value","spans":[{"byte_end":4884,"byte_start":4866,"column_end":31,"column_start":13,"expansion":null,"file_name":"xtask/src/main.rs","is_primary":true,"label":null,"line_end":114,"line_start":114,"suggested_replacement":null,"suggestion_applicability":null,"text":[{"highlight_end":31,"highlight_start":13,"text":"    let _ = Some(1u8).unwrap();"}]}],"code":{"code":"clippy::unnecessary_literal_unwrap","explanation":null}}}"#;

/// Real clippy 1.99.0 output for `/// Uses FooBar for things.` in a scratch crate, run as
/// `cargo clippy --message-format=json-diagnostic-short -- -W clippy::doc_markdown -D warnings`.
/// Its children are the docs link, `implied by -D warnings`, an `#[allow]` suggestion and a
/// `try` suggestion. Cut: `package_id`, `manifest_path` and `target`.
const DOC_MARKDOWN: &str = r#"{"reason":"compiler-message","message":{"rendered":"src/lib.rs:3:10: error: item in documentation is missing backticks\n","$message_type":"diagnostic","children":[{"children":[],"code":null,"level":"help","message":"for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#doc_markdown","rendered":null,"spans":[]},{"children":[],"code":null,"level":"note","message":"`-D clippy::doc-markdown` implied by `-D warnings`","rendered":null,"spans":[]},{"children":[],"code":null,"level":"help","message":"to override `-D warnings` add `#[allow(clippy::doc_markdown)]`","rendered":null,"spans":[]},{"children":[],"code":null,"level":"help","message":"try","rendered":null,"spans":[{"byte_end":51,"byte_start":45,"column_end":16,"column_start":10,"expansion":null,"file_name":"src/lib.rs","is_primary":true,"label":null,"line_end":3,"line_start":3,"suggested_replacement":"`FooBar`","suggestion_applicability":"MachineApplicable","text":[{"highlight_end":16,"highlight_start":10,"text":"/// Uses FooBar for things."}]}]}],"level":"error","message":"item in documentation is missing backticks","spans":[{"byte_end":51,"byte_start":45,"column_end":16,"column_start":10,"expansion":null,"file_name":"src/lib.rs","is_primary":true,"label":null,"line_end":3,"line_start":3,"suggested_replacement":null,"suggestion_applicability":null,"text":[{"highlight_end":16,"highlight_start":10,"text":"/// Uses FooBar for things."}]}],"code":{"code":"clippy::doc_markdown","explanation":null}}}"#;

#[test]
fn a_ban_prints_its_reason_and_its_replacement() {
    assert_eq!(
        short_diags(CANONICALIZE, "/ws"),
        vec![Diag {
            path: "xtask/src/main.rs".into(),
            line: 113,
            message: "error: use of a disallowed method `std::fs::canonicalize` (clippy::disallowed_methods): canonicalize returns \\\\?\\ paths on Windows that most tools refuse; use `dunce::canonicalize`".into(),
            is_error: true,
        }]
    );
}

#[test]
fn a_ban_with_no_replacement_prints_its_reason_from_the_note() {
    assert_eq!(
        short_diags(WRITE, "/ws"),
        vec![Diag {
            path: "xtask/src/main.rs".into(),
            line: 114,
            message: "error: use of a disallowed method `std::fs::write` (clippy::disallowed_methods): files change only through the crate's write module: xtask/src/write.rs in xtask, Scratch (xtask/src/scratch.rs) in xtask's tests; a crate that writes its first file adds its own, with one #[expect] listed in xtask/expect-allowlist.txt".into(),
            is_error: true,
        }]
    );
}

#[test]
fn a_diagnostic_with_only_boilerplate_notes_prints_as_before() {
    assert_eq!(
        short_diags(EQ_OP, "/ws"),
        vec![Diag {
            path: "xtask/src/main.rs".into(),
            line: 115,
            message: "error: equal expressions as operands to `==` (clippy::eq_op)".into(),
            is_error: true,
        }]
    );
}

#[test]
fn a_lint_that_is_not_a_ban_prints_no_notes_even_with_a_help_child() {
    assert_eq!(
        short_diags(LET_UNDERSCORE, "/ws"),
        vec![Diag {
            path: "xtask/src/main.rs".into(),
            line: 115,
            message: "error: non-binding `let` on an expression with `#[must_use]` type (clippy::let_underscore_must_use)".into(),
            is_error: true,
        }]
    );
}

#[test]
fn a_lint_that_is_not_a_ban_prints_no_suggestion_either() {
    assert_eq!(
        short_diags(UNWRAP_SOME, "/ws"),
        vec![Diag {
            path: "xtask/src/main.rs".into(),
            line: 114,
            message:
                "warning: used `unwrap()` on `Some` value (clippy::unnecessary_literal_unwrap)"
                    .into(),
            is_error: false,
        }]
    );
}

#[test]
fn a_pedantic_lint_under_dash_d_warnings_gets_no_implied_note_and_no_allow() {
    assert_eq!(
        short_diags(DOC_MARKDOWN, "/ws"),
        vec![Diag {
            path: "src/lib.rs".into(),
            line: 3,
            message: "error: item in documentation is missing backticks (clippy::doc_markdown)"
                .into(),
            is_error: true,
        }]
    );
}

/// Hand-written in the shape clippy prints: `text` is the message, `code` the lint, and each of
/// `children` a JSON note or help.
fn hand_written(text: &str, code: &str, children: &[&str]) -> String {
    format!(
        r#"{{"reason":"compiler-message","message":{{"rendered":"xtask/src/main.rs:113:13: error: {text}\n","children":[{children}],"level":"error","message":"{text}","spans":[],"code":{{"code":"{code}","explanation":null}}}}}}"#,
        children = children.join(",")
    )
}

/// The one message `short_diags` makes from `output`.
fn message_of(output: &str) -> String {
    let diags = short_diags(output, "/ws");
    assert_eq!(diags.len(), 1, "one diagnostic expected: {diags:?}");
    diags[0].message.clone()
}

#[test]
fn a_ban_never_prints_the_lint_level_note_or_an_allow_suggestion() {
    let implied = r#"{"children":[],"code":null,"level":"note","message":"`-D clippy::disallowed-methods` implied by `-D warnings`","rendered":null,"spans":[]}"#;
    let allow = r#"{"children":[],"code":null,"level":"help","message":"to override `-D warnings` add `#[allow(clippy::disallowed_methods)]`","rendered":null,"spans":[]}"#;
    let reason = r#"{"children":[],"code":null,"level":"help","message":"canonicalize returns paths that most tools refuse","rendered":null,"spans":[{"suggested_replacement":"dunce::canonicalize"}]}"#;
    let line = hand_written(
        "use of a disallowed method `std::fs::canonicalize`",
        "clippy::disallowed_methods",
        &[implied, allow, reason],
    );
    assert_eq!(
        message_of(&line),
        "error: use of a disallowed method `std::fs::canonicalize` (clippy::disallowed_methods): canonicalize returns paths that most tools refuse; use `dunce::canonicalize`"
    );
}

#[test]
fn a_disallowed_type_prints_its_note_and_no_note_that_points_at_code() {
    let docs = r#"{"children":[],"code":null,"level":"help","message":"for further information visit https://rust-lang.github.io/rust-clippy/rust-1.99.0/index.html#disallowed_types","rendered":null,"spans":[]}"#;
    let points_at_type = r#"{"children":[],"code":null,"level":"note","message":"type is `std::path::PathBuf`","rendered":null,"spans":[{"suggested_replacement":null}]}"#;
    let reason = r#"{"children":[],"code":null,"level":"note","message":"paths come from the workspace root only","rendered":null,"spans":[]}"#;
    let line = hand_written(
        "use of a disallowed type `std::path::PathBuf`",
        "clippy::disallowed_types",
        &[docs, points_at_type, reason],
    );
    assert_eq!(
        message_of(&line),
        "error: use of a disallowed type `std::path::PathBuf` (clippy::disallowed_types): paths come from the workspace root only"
    );
}

#[test]
fn an_empty_replacement_adds_no_use_piece() {
    let reason = r#"{"children":[],"code":null,"level":"help","message":"canonicalize returns paths that most tools refuse","rendered":null,"spans":[{"suggested_replacement":""}]}"#;
    let line = hand_written(
        "use of a disallowed method `std::fs::canonicalize`",
        "clippy::disallowed_methods",
        &[reason],
    );
    assert_eq!(
        message_of(&line),
        "error: use of a disallowed method `std::fs::canonicalize` (clippy::disallowed_methods): canonicalize returns paths that most tools refuse"
    );
}

#[test]
fn clippy_render_prints_the_rendered_line_and_no_help_notes() {
    assert_eq!(
        clippy_render(CANONICALIZE),
        "xtask/src/main.rs:113:13: error: use of a disallowed method `std::fs::canonicalize`\n"
    );
}
