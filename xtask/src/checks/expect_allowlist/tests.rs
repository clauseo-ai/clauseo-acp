//! Tests of the expect-allowlist check: the builders of its planted cases (in `tests/planted.rs`)
//! and the fix texts. Each planted case writes its files into a scratch git repo, adds them, and
//! the planted harness runs the check on it.

mod cases;
pub(super) mod planted;

pub(super) use cases::*;

use super::*;
use crate::checks::planted::{Repo, git};
use crate::scratch::Scratch;

/// The allowlist's first line in every case, so its entries start at line 2.
const HEADER: &str = "# expect allowlist\n";

/// A scratch git repo named after `case` that tracks each `(path, text)` of
/// `files` and leaves each of `untracked` untracked.
fn run_files(case: &str, files: &[(&str, &str)], untracked: &[(&str, &str)]) -> Repo {
    let dir = Scratch::new(&format!("planted-expect-allowlist-{case}"));
    git(&dir, &["init", "-q"]);
    let write = |(path, text): &(&str, &str)| {
        let full = dir.join(path);
        Scratch::create_dir_all(full.parent().expect("a file has a parent")).expect("create dirs");
        Scratch::write(full, text).expect("write a file");
    };
    files.iter().for_each(write);
    git(&dir, &["add", "-A"]);
    untracked.iter().for_each(write);
    Repo::new(dir)
}

/// A repo whose one Rust file is `src/a.rs`, and whose allowlist is `HEADER`
/// and `entries`.
fn run_on(case: &str, rust: &str, entries: &str) -> Repo {
    let list = format!("{HEADER}{entries}");
    run_files(case, &[("src/a.rs", rust), (ALLOWLIST, &list)], &[])
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

/// A tracked `.rs` file that is not UTF-8: it is unread, so no site is counted.
pub(super) fn rust_file_not_utf8() -> Repo {
    let dir = Scratch::new("planted-expect-allowlist-rust-file-not-utf8");
    git(&dir, &["init", "-q"]);
    Scratch::create_dir_all(dir.join("xtask")).expect("create the folder");
    Scratch::write(dir.join(ALLOWLIST), HEADER).expect("write the allowlist");
    Scratch::create_dir_all(dir.join("src")).expect("create the folder");
    Scratch::write(dir.join("src/a.rs"), b"fn f() {}\xff\n").expect("write a.rs");
    git(&dir, &["add", "-A"]);
    Repo::new(dir)
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
