//! Tests of the expect-allowlist check: the builders of its planted cases (in `tests/planted.rs`)
//! and the fix texts. Each planted case writes its files into a scratch git repo, adds them, and
//! the planted harness runs the check on it.

pub(super) mod planted;

use super::*;
use crate::checks::planted::{Repo, git};
use crate::scratch::Scratch;
use std::mem::discriminant;

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
        "struct S {\n    #[expect(dead_code, reason = \"x\")]\n    x: u8,\n    \
         #[expect(dead_code, reason = \"x\")]\n    y: u8,\n}\n",
        "src/a.rs a dead_code\n",
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

/// One sample of every `Fix` variant, each a different variant.
fn samples() -> Vec<Fix> {
    let (path, item, lint) = (
        "src/a.rs".to_owned(),
        "f".to_owned(),
        "dead_code".to_owned(),
    );
    vec![
        Fix::Unlisted {
            path: path.clone(),
            item: item.clone(),
            lint: lint.clone(),
        },
        Fix::Stale { line: 7 },
        Fix::Allow {
            path,
            item,
            lint,
            inner: true,
        },
        Fix::EmptyAllow,
        Fix::Malformed,
        Fix::Unparsable,
        Fix::MissingAllowlist,
    ]
}

/// The exact text of every fix. The `match` has no `_` arm, so a new variant does not compile
/// until its text is pinned here.
#[test]
fn every_fix_has_its_exact_text() {
    for fix in samples() {
        let expected = match &fix {
            Fix::Unlisted { .. } => {
                "remove the #[expect], or add \"src/a.rs f dead_code\" to \
                 xtask/expect-allowlist.txt (the code owner approves it)"
            }
            Fix::Stale { .. } => "delete line 7 of xtask/expect-allowlist.txt",
            Fix::Allow { .. } => {
                "delete the allow, or write #![expect(dead_code, reason = \"...\")] in its place \
                 and add \"src/a.rs f dead_code\" to xtask/expect-allowlist.txt (the code owner \
                 approves it)"
            }
            Fix::EmptyAllow => "delete the allow: it names no lint",
            Fix::Malformed => {
                "write the line as \"<path> <item> <lint>\", three fields separated by spaces, \
                 as the comment at the top of xtask/expect-allowlist.txt says, or delete it"
            }
            Fix::Unparsable => {
                "make the file valid Rust that syn can parse: expect-allowlist reads every .rs \
                 file in the repo for its #[expect] and #[allow] sites"
            }
            Fix::MissingAllowlist => {
                "restore xtask/expect-allowlist.txt, one \"<path> <item> <lint>\" line per \
                 #[expect] site; the check matches the sites against it only once it is there"
            }
        };
        assert_eq!(fix.to_string(), expected);
    }
}

#[test]
fn the_fix_samples_are_distinct_variants() {
    let samples = samples();
    for (index, fix) in samples.iter().enumerate() {
        for other in &samples[index + 1..] {
            assert_ne!(
                discriminant(fix),
                discriminant(other),
                "{fix:?} and {other:?}"
            );
        }
    }
}

#[test]
fn an_outer_allow_names_the_outer_form_in_its_fix() {
    let fix = Fix::Allow {
        path: "src/a.rs".to_owned(),
        item: "f".to_owned(),
        lint: "unused".to_owned(),
        inner: false,
    };
    assert_eq!(
        fix.to_string(),
        "delete the allow, or write #[expect(unused, reason = \"...\")] in its place and add \
         \"src/a.rs f unused\" to xtask/expect-allowlist.txt (the code owner approves it)"
    );
}
