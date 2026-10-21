//! The text of each `expect-allowlist` fix, and the fix of an outer warn.

use super::{Fix, Level};
use crate::fix::tests::{assert_texts, fix_texts};

fix_texts! {
    fn pinned for Fix {
        Fix::Unlisted { .. } => {
            "remove the #[expect], or add \"src/a.rs f dead_code\" to \
             xtask/expect-allowlist.txt (the code owner approves it)"
        },
        Fix::Stale { .. } => "delete line 7 of xtask/expect-allowlist.txt",
        Fix::Lower { .. } => {
            "delete the allow, or write #![expect(dead_code, reason = \"...\")] in its place \
             and add \"src/a.rs f dead_code\" to xtask/expect-allowlist.txt (the code owner \
             approves it)"
        },
        Fix::EmptyLower { .. } => "delete the warn: it names no lint",
        Fix::MetavariableAttribute => {
            "write the attribute's name and each of its lints in the macro body itself, with \
             no metavariable, so expect-allowlist can read them; a caller cannot pass in an \
             attribute or a lint"
        },
        Fix::HashWithoutBrackets => {
            "write the attribute's brackets right after its `#`, as `#[..]` or `#![..]`, \
             with its name and lints in the macro body itself"
        },
        Fix::Path => {
            "give #[path] a string literal naming a .rs file that git lists, relative to \
             this file's folder, on a module at the top level of the file, or move the \
             module to its default path; expect-allowlist reads only those files"
        },
        Fix::Include => {
            "move the code into a module, or include! only a .rs file that git lists, named \
             by a string literal relative to this file's folder; expect-allowlist reads only \
             those files"
        },
        Fix::Malformed => {
            "write the line as \"<path> <item> <lint>\", three fields separated by spaces, \
             as the comment at the top of xtask/expect-allowlist.txt says, or delete it"
        },
        Fix::Unparsable => {
            "make the file valid Rust that syn can parse: expect-allowlist reads every .rs \
             file in the repo for its expect, allow and warn attributes"
        },
        Fix::MissingAllowlist => {
            "restore xtask/expect-allowlist.txt, one \"<path> <item> <lint>\" line per \
             #[expect] site; the check matches the sites against it only once it is there"
        },
    }
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
        Fix::Lower {
            level: Level::Allow,
            path,
            item,
            lint,
            inner: true,
        },
        Fix::EmptyLower { level: Level::Warn },
        Fix::MetavariableAttribute,
        Fix::HashWithoutBrackets,
        Fix::Path,
        Fix::Include,
        Fix::Malformed,
        Fix::Unparsable,
        Fix::MissingAllowlist,
    ]
}

/// The text of every fix: `fix_texts!` numbers the arms, and `assert_texts` fails until each
/// arm has a sample.
#[test]
fn every_fix_has_its_exact_text() {
    assert_texts(&samples(), pinned, ARMS);
}

#[test]
fn an_outer_warn_names_the_outer_form_and_the_warn_in_its_fix() {
    let fix = Fix::Lower {
        level: Level::Warn,
        path: "src/a.rs".to_owned(),
        item: "f".to_owned(),
        lint: "unused".to_owned(),
        inner: false,
    };
    assert_eq!(
        fix.to_string(),
        "delete the warn, or write #[expect(unused, reason = \"...\")] in its place and add \
         \"src/a.rs f unused\" to xtask/expect-allowlist.txt (the code owner approves it)"
    );
}
