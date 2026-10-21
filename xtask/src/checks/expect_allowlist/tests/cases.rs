//! The builders of the planted cases for `warn`, for attributes a macro body hides, and for
//! `#[path]` and `include!`: the holes closed after review of PR #18.

use super::{HEADER, run_files, run_on};
use crate::checks::planted::Repo;

pub(in crate::checks::expect_allowlist) fn warn_outer() -> Repo {
    run_on(
        "warn-outer",
        "#[warn(clippy::disallowed_methods)]\nfn f() {}\n",
        "",
    )
}

pub(in crate::checks::expect_allowlist) fn warn_inner_at_file_top() -> Repo {
    run_on(
        "warn-inner-at-file-top",
        "#![warn(warnings)]\nfn f() {}\n",
        "",
    )
}

pub(in crate::checks::expect_allowlist) fn warn_cfg_attr() -> Repo {
    run_on(
        "warn-cfg-attr",
        "fn f() {\n    #[cfg_attr(unix, cfg_attr(all(), warn(unused)))]\n    let x = 1;\n}\n",
        "",
    )
}

pub(in crate::checks::expect_allowlist) fn warn_in_macro_rules() -> Repo {
    run_on(
        "warn-in-macro-rules",
        "macro_rules! m {\n    () => {\n        #![warn(dead_code)]\n    };\n}\n",
        "",
    )
}

pub(in crate::checks::expect_allowlist) fn warn_in_macro_call() -> Repo {
    run_on(
        "warn-in-macro-call",
        "fn f() {\n    m! {\n        #[warn(dead_code)]\n        fn g() {}\n    }\n}\n",
        "",
    )
}

pub(in crate::checks::expect_allowlist) fn warn_with_no_lint() -> Repo {
    run_on("warn-with-no-lint", "#[warn()]\nfn f() {}\n", "")
}

pub(in crate::checks::expect_allowlist) fn deny_and_forbid() -> Repo {
    run_on(
        "deny-and-forbid",
        "#![forbid(unsafe_code)]\n#[deny(clippy::disallowed_methods)]\nfn f() {}\n",
        "",
    )
}

/// A macro whose body writes `body` as its one arm's transcriber, at line 3.
fn macro_with(case: &str, body: &str) -> Repo {
    let text = format!(
        "macro_rules! q {{\n    ($m:meta, $n:ident, $($t:tt)*) => {{\n        {body}\n        fn a() {{}}\n    }};\n}}\n"
    );
    run_on(case, &text, "")
}

pub(in crate::checks::expect_allowlist) fn metavariable_attribute() -> Repo {
    macro_with("metavariable-attribute", "#[$m]")
}

pub(in crate::checks::expect_allowlist) fn metavariable_name_with_lints() -> Repo {
    macro_with("metavariable-name-with-lints", "#[$n(dead_code)]")
}

pub(in crate::checks::expect_allowlist) fn metavariable_in_cfg_attr() -> Repo {
    macro_with("metavariable-in-cfg-attr", "#[cfg_attr(all(), $m)]")
}

pub(in crate::checks::expect_allowlist) fn metavariable_inner_repetition() -> Repo {
    macro_with("metavariable-inner-repetition", "#![$($t)*]")
}

pub(in crate::checks::expect_allowlist) fn metavariable_lint_in_expect() -> Repo {
    macro_with(
        "metavariable-lint-in-expect",
        "#[expect($n, reason = \"x\")]",
    )
}

pub(in crate::checks::expect_allowlist) fn hash_without_brackets() -> Repo {
    run_on(
        "hash-without-brackets",
        "macro_rules! q {\n    ($b:tt) => {\n        # $b\n        fn a() {}\n    };\n}\n",
        "",
    )
}

/// `src/a.rs` declares `mod_line`; `others` are the other files, all tracked.
fn with_files(case: &str, rust: &str, others: &[(&str, &str)]) -> Repo {
    let mut files = vec![("src/a.rs", rust), ("xtask/expect-allowlist.txt", HEADER)];
    files.extend_from_slice(others);
    run_files(case, &files, &[])
}

pub(in crate::checks::expect_allowlist) fn path_to_listed_rs() -> Repo {
    with_files(
        "path-to-listed-rs",
        "#[path = \"b/c.rs\"]\nmod c;\n",
        &[("src/b/c.rs", "fn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist) fn path_to_non_rs() -> Repo {
    with_files(
        "path-to-non-rs",
        "#[path = \"x.txt\"]\nmod x;\n",
        &[("src/x.txt", "#[allow(dead_code)]\nfn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist) fn path_to_ignored_rs() -> Repo {
    run_files(
        "path-to-ignored-rs",
        &[
            (
                "src/a.rs",
                "#[cfg_attr(unix, path = \"gen/x.rs\")]\nmod x;\n",
            ),
            ("xtask/expect-allowlist.txt", HEADER),
            (".gitignore", "src/gen/\n"),
        ],
        &[("src/gen/x.rs", "#![allow(dead_code)]\n")],
    )
}

pub(in crate::checks::expect_allowlist) fn path_outside_repo() -> Repo {
    with_files(
        "path-outside-repo",
        "#[path = \"../../x.rs\"]\nmod x;\n",
        &[],
    )
}

pub(in crate::checks::expect_allowlist) fn path_in_inline_mod() -> Repo {
    with_files(
        "path-in-inline-mod",
        "mod outer {\n    #[path = \"c.rs\"]\n    mod c;\n}\n",
        &[("src/c.rs", "fn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist) fn include_listed_rs() -> Repo {
    with_files(
        "include-listed-rs",
        "mod m {\n    include!(\"../gen/b.rs\");\n}\n",
        &[("gen/b.rs", "fn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist) fn include_non_rs() -> Repo {
    with_files(
        "include-non-rs",
        "include!(\"x.inc\");\n",
        &[("src/x.inc", "#[allow(dead_code)]\nfn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist) fn include_not_a_literal() -> Repo {
    with_files(
        "include-not-a-literal",
        "include!(concat!(env!(\"OUT_DIR\"), \"/x.rs\"));\n",
        &[],
    )
}

pub(in crate::checks::expect_allowlist) fn include_in_macro_body() -> Repo {
    with_files(
        "include-in-macro-body",
        "macro_rules! i {\n    () => {\n        include!(\"x.inc\");\n    };\n}\n",
        &[],
    )
}
