//! The builders of the expect-allowlist planted cases. Each writes a scratch git repo whose
//! files are tracked by git, as the check reads them, except those `run_files` leaves untracked.

use crate::checks::expect_allowlist::ALLOWLIST;
use crate::checks::planted::{Repo, git, scratch};

/// The allowlist's first line in every case, so its entries start at line 2.
const HEADER: &str = "# expect allowlist\n";

/// A scratch git repo named after `case` that tracks each `(path, bytes)` of `files` and leaves
/// each of `untracked` untracked.
fn run_files<T: AsRef<[u8]>>(case: &str, files: &[(&str, T)], untracked: &[(&str, T)]) -> Repo {
    let written: Vec<(&str, &[u8])> = files
        .iter()
        .chain(untracked)
        .map(|(path, bytes)| (*path, bytes.as_ref()))
        .collect();
    let dir = scratch(&format!("expect-allowlist-{case}"), &written);
    git(&dir, &["init", "-q"]);
    let mut add = vec!["add", "--"];
    add.extend(files.iter().map(|(path, _)| *path));
    git(&dir, &add);
    Repo::new(dir)
}

/// A repo whose one Rust file is `src/a.rs`, and whose allowlist is `HEADER` and `entries`.
fn run_on(case: &str, rust: &str, entries: &str) -> Repo {
    let list = format!("{HEADER}{entries}");
    run_files(case, &[("src/a.rs", rust), (ALLOWLIST, list.as_str())], &[])
}

pub(super) fn listed_site() -> Repo {
    run_on(
        "listed-site",
        "fn f() {}\n#[expect(dead_code, reason = \"x\")]\nfn g() {}\n",
        "src/a.rs g dead_code\n",
    )
}

pub(super) fn doc_and_string_text() -> Repo {
    run_on(
        "doc-and-string-text",
        "//! Uses `#[expect(dead_code)]`.\n/// And `#![allow(unused)]`.\n\
         fn f() -> &'static str {\n    \"#[expect(dead_code)]\"\n}\n",
        "",
    )
}

pub(super) fn outer() -> Repo {
    run_on(
        "outer",
        "fn f() {}\n#[expect(dead_code, reason = \"x\")]\nfn g() {}\n",
        "",
    )
}

pub(super) fn inner_at_file_top() -> Repo {
    run_on(
        "inner-at-file-top",
        "#![expect(unsafe_code, reason = \"x\")]\nfn f() {}\n",
        "",
    )
}

pub(super) fn inner_in_fn() -> Repo {
    run_on(
        "inner-in-fn",
        "fn f() {\n    #![expect(unused, reason = \"x\")]\n}\n",
        "",
    )
}

pub(super) fn cfg_attr() -> Repo {
    run_on(
        "cfg-attr",
        "#[cfg_attr(test, expect(dead_code, reason = \"x\"))]\nfn f() {}\n",
        "",
    )
}

pub(super) fn nested_cfg_attr() -> Repo {
    run_on(
        "nested-cfg-attr",
        "struct S;\n#[cfg_attr(unix, cfg_attr(test, expect(dead_code, reason = \"x\")))]\n\
         const C: u8 = 1;\n",
        "",
    )
}

pub(super) fn several_lints() -> Repo {
    run_on(
        "several-lints",
        "#[expect(dead_code, clippy::unwrap_used, reason = \"x\")]\nfn f() {}\n",
        "src/a.rs f dead_code\n",
    )
}

pub(super) fn macro_rules_body() -> Repo {
    run_on(
        "macro-rules-body",
        "macro_rules! m {\n    () => {\n        #[expect(dead_code, reason = \"x\")]\n        \
         fn g() {}\n    };\n}\n",
        "",
    )
}

pub(super) fn macro_call_body() -> Repo {
    run_on(
        "macro-call-body",
        "fn f() {\n    m! {\n        #[expect(dead_code, reason = \"x\")]\n        \
         fn g() {}\n    }\n}\n",
        "",
    )
}

pub(super) fn stale_line() -> Repo {
    run_on(
        "stale-line",
        "fn f() {}\n",
        "src/a.rs f dead_code\nsrc/gone.rs - unsafe_code\n",
    )
}

pub(super) fn two_sites_one_line() -> Repo {
    run_on(
        "two-sites-one-line",
        "fn f() {\n    #[expect(unused, reason = \"x\")]\n    let x = 1;\n    \
         #[expect(unused, reason = \"x\")]\n    let y = 1;\n}\n",
        "src/a.rs f unused\n",
    )
}

pub(super) fn one_site_two_lines() -> Repo {
    run_on(
        "one-site-two-lines",
        "#[expect(dead_code, reason = \"x\")]\nfn f() {}\n",
        "src/a.rs f dead_code\nsrc/a.rs f dead_code\n",
    )
}

pub(super) fn allow_outer() -> Repo {
    run_on("allow-outer", "#[allow(dead_code)]\nfn f() {}\n", "")
}

pub(super) fn allow_inner_at_file_top() -> Repo {
    run_on(
        "allow-inner-at-file-top",
        "#![allow(clippy::disallowed_methods)]\nfn f() {}\n",
        "",
    )
}

pub(super) fn allow_inner_in_mod() -> Repo {
    run_on(
        "allow-inner-in-mod",
        "mod m {\n    #![allow(dead_code)]\n}\n",
        "",
    )
}

pub(super) fn allow_cfg_attr() -> Repo {
    run_on(
        "allow-cfg-attr",
        "#![cfg_attr(all(), allow(clippy::disallowed_methods))]\nfn f() {}\n",
        "",
    )
}

pub(super) fn allow_nested_cfg_attr() -> Repo {
    run_on(
        "allow-nested-cfg-attr",
        "fn f() {\n    #[cfg_attr(unix, cfg_attr(all(), allow(unused)))]\n    let x = 1;\n}\n",
        "",
    )
}

pub(super) fn allow_in_macro_rules() -> Repo {
    run_on(
        "allow-in-macro-rules",
        "macro_rules! m {\n    () => {\n        #![allow(dead_code)]\n    };\n}\n",
        "",
    )
}

pub(super) fn allow_in_macro_call() -> Repo {
    run_on(
        "allow-in-macro-call",
        "fn f() {\n    m! {\n        #[allow(dead_code)]\n        fn g() {}\n    }\n}\n",
        "",
    )
}

pub(super) fn cfg_attr_in_macro_call() -> Repo {
    run_on(
        "cfg-attr-in-macro-call",
        "fn f() {\n    m!(#[cfg_attr(test, expect(dead_code, reason = \"x\"))] fn g() {});\n}\n",
        "",
    )
}

pub(super) fn allow_bare_path() -> Repo {
    run_on("allow-bare-path", "#[allow]\nfn f() {}\n", "")
}

pub(super) fn allow_with_no_lint() -> Repo {
    run_on("allow-with-no-lint", "#[allow()]\nfn f() {}\n", "")
}

pub(super) fn malformed_line() -> Repo {
    run_on(
        "malformed-line",
        "fn f() {}\n",
        "src/a.rs dead_code\nsrc/a.rs f dead_code extra\n",
    )
}

pub(super) fn unparsable_file() -> Repo {
    run_on(
        "unparsable-file",
        "fn f() {}\nfn g() -> {}\n",
        "src/a.rs g dead_code\n",
    )
}

pub(super) fn missing_allowlist() -> Repo {
    run_files(
        "missing-allowlist",
        &[(
            "src/a.rs",
            "#[expect(dead_code, reason = \"x\")]\nfn f() {}\n",
        )],
        &[],
    )
}

pub(super) fn untracked_upper_case_rs() -> Repo {
    run_files(
        "untracked-upper-case-rs",
        &[(ALLOWLIST, HEADER)],
        &[(
            "src/B.RS",
            "#[expect(dead_code, reason = \"x\")]\nfn f() {}\n",
        )],
    )
}

pub(super) fn warn_outer() -> Repo {
    run_on(
        "warn-outer",
        "#[warn(clippy::disallowed_methods)]\nfn f() {}\n",
        "",
    )
}

pub(super) fn warn_inner_at_file_top() -> Repo {
    run_on(
        "warn-inner-at-file-top",
        "#![warn(warnings)]\nfn f() {}\n",
        "",
    )
}

pub(super) fn warn_cfg_attr() -> Repo {
    run_on(
        "warn-cfg-attr",
        "fn f() {\n    #[cfg_attr(unix, cfg_attr(all(), warn(unused)))]\n    let x = 1;\n}\n",
        "",
    )
}

pub(super) fn warn_in_macro_rules() -> Repo {
    run_on(
        "warn-in-macro-rules",
        "macro_rules! m {\n    () => {\n        #![warn(dead_code)]\n    };\n}\n",
        "",
    )
}

pub(super) fn warn_in_macro_call() -> Repo {
    run_on(
        "warn-in-macro-call",
        "fn f() {\n    m! {\n        #[warn(dead_code)]\n        fn g() {}\n    }\n}\n",
        "",
    )
}

pub(super) fn warn_with_no_lint() -> Repo {
    run_on("warn-with-no-lint", "#[warn()]\nfn f() {}\n", "")
}

pub(super) fn deny_and_forbid() -> Repo {
    run_on(
        "deny-and-forbid",
        "#![forbid(unsafe_code)]\n#[deny(clippy::disallowed_methods)]\nfn f() {}\n",
        "",
    )
}

/// A macro whose body writes `body` as its one arm's transcriber, at line 3.
fn macro_with(case: &str, body: &str) -> Repo {
    let text = format!(
        "macro_rules! q {{\n    ($m:meta, $n:ident, $($t:tt)*) => {{\n        {body}\n        \
         fn a() {{}}\n    }};\n}}\n"
    );
    run_on(case, &text, "")
}

pub(super) fn metavariable_attribute() -> Repo {
    macro_with("metavariable-attribute", "#[$m]")
}

pub(super) fn metavariable_name_with_lints() -> Repo {
    macro_with("metavariable-name-with-lints", "#[$n(dead_code)]")
}

pub(super) fn metavariable_in_cfg_attr() -> Repo {
    macro_with("metavariable-in-cfg-attr", "#[cfg_attr(all(), $m)]")
}

pub(super) fn metavariable_inner_repetition() -> Repo {
    macro_with("metavariable-inner-repetition", "#![$($t)*]")
}

pub(super) fn metavariable_lint_in_expect() -> Repo {
    macro_with(
        "metavariable-lint-in-expect",
        "#[expect($n, reason = \"x\")]",
    )
}

pub(super) fn hash_without_brackets() -> Repo {
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

pub(super) fn path_to_listed_rs() -> Repo {
    with_files(
        "path-to-listed-rs",
        "#[path = \"b/c.rs\"]\nmod c;\n",
        &[("src/b/c.rs", "fn g() {}\n")],
    )
}

pub(super) fn path_to_non_rs() -> Repo {
    with_files(
        "path-to-non-rs",
        "#[path = \"x.txt\"]\nmod x;\n",
        &[("src/x.txt", "#[allow(dead_code)]\nfn g() {}\n")],
    )
}

pub(super) fn path_to_ignored_rs() -> Repo {
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

pub(super) fn path_outside_repo() -> Repo {
    with_files(
        "path-outside-repo",
        "#[path = \"../../x.rs\"]\nmod x;\n",
        &[],
    )
}

pub(super) fn path_in_inline_mod() -> Repo {
    with_files(
        "path-in-inline-mod",
        "mod outer {\n    #[path = \"c.rs\"]\n    mod c;\n}\n",
        &[("src/c.rs", "fn g() {}\n")],
    )
}

pub(super) fn include_listed_rs() -> Repo {
    with_files(
        "include-listed-rs",
        "mod m {\n    include!(\"../gen/b.rs\");\n}\n",
        &[("gen/b.rs", "fn g() {}\n")],
    )
}

pub(super) fn include_non_rs() -> Repo {
    with_files(
        "include-non-rs",
        "include!(\"x.inc\");\n",
        &[("src/x.inc", "#[allow(dead_code)]\nfn g() {}\n")],
    )
}

pub(super) fn include_not_a_literal() -> Repo {
    with_files(
        "include-not-a-literal",
        "include!(concat!(env!(\"OUT_DIR\"), \"/x.rs\"));\n",
        &[],
    )
}

pub(super) fn include_in_macro_body() -> Repo {
    with_files(
        "include-in-macro-body",
        "macro_rules! i {\n    () => {\n        include!(\"x.inc\");\n    };\n}\n",
        &[],
    )
}

/// A tracked `.rs` file that is not UTF-8: it is unread, so no site is counted.
pub(super) fn rust_file_not_utf8() -> Repo {
    run_files(
        "rust-file-not-utf8",
        &[
            ("src/a.rs", b"fn f() {}\xff\n".as_slice()),
            (ALLOWLIST, HEADER.as_bytes()),
        ],
        &[],
    )
}

/// A repo with no Rust file, whose allowlist has one entry: that entry is stale, and the zero
/// count is still reported beside it.
pub(super) fn no_rust_file() -> Repo {
    run_files(
        "no-rust-file",
        &[(ALLOWLIST, "# expect allowlist\nsrc/a.rs f dead_code\n")],
        &[],
    )
}
