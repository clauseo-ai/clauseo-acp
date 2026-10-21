//! The builders of the expect-allowlist planted cases with `#[path]`, `include!`, attribute values
//! and other tracked files. Each goes through `with_files`, which tracks the files it writes.

use super::{HEADER, run_files, run_on};
use crate::checks::expect_allowlist::ALLOWLIST;
use crate::checks::planted::Repo;

/// `src/a.rs` is `rust`; `others` are the other files, all tracked.
fn with_files(case: &str, rust: &str, others: &[(&str, &str)]) -> Repo {
    let mut files = vec![("src/a.rs", rust), ("xtask/expect-allowlist.txt", HEADER)];
    files.extend_from_slice(others);
    run_files(case, &files, &[])
}

pub(in crate::checks::expect_allowlist::tests) fn path_to_listed_rs() -> Repo {
    with_files(
        "path-to-listed-rs",
        "#[path = \"b/c.rs\"]\nmod c;\n",
        &[("src/b/c.rs", "fn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn path_to_non_rs() -> Repo {
    with_files(
        "path-to-non-rs",
        "#[path = \"x.txt\"]\nmod x;\n",
        &[("src/x.txt", "#[allow(dead_code)]\nfn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn path_to_ignored_rs() -> Repo {
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

pub(in crate::checks::expect_allowlist::tests) fn path_dot_segment() -> Repo {
    with_files(
        "path-dot-segment",
        "#[path = \"./b.rs\"]\nmod b;\n",
        &[("src/b.rs", "fn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn attribute_value_include() -> Repo {
    with_files(
        "attribute-value-include",
        "#[doc = include!(\"x.inc\")]\nstruct S;\n",
        &[],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn attribute_value_allow() -> Repo {
    with_files(
        "attribute-value-allow",
        "#[doc = { #[allow(dead_code)] let _a = 1; \"s\" }]\nstruct S;\n",
        &[],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn attribute_value_expect_listed() -> Repo {
    run_on(
        "attribute-value-expect-listed",
        "#[doc = { #[expect(dead_code, reason = \"x\")] let _a = 1; \"s\" }]\nstruct S;\n",
        "src/a.rs S dead_code\n",
    )
}

pub(in crate::checks::expect_allowlist::tests) fn attribute_value_include_in_macro() -> Repo {
    with_files(
        "attribute-value-include-in-macro",
        concat!(
            "macro_rules! d {\n    () => {\n",
            "        #[doc = include!(\"x.inc\")]\n        struct S;\n    };\n}\n",
        ),
        &[],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn attribute_value_allow_in_macro() -> Repo {
    with_files(
        "attribute-value-allow-in-macro",
        concat!(
            "macro_rules! d {\n    () => {\n",
            "        #[doc = { #[allow(dead_code)] let _a = 1; \"s\" }]\n",
            "        struct S;\n    };\n}\n",
        ),
        &[],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn path_drive_letter() -> Repo {
    with_files(
        "path-drive-letter",
        "#[path = \"C:/abs/x.rs\"]\nmod x;\n",
        &[],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn path_backslash() -> Repo {
    with_files("path-backslash", "#[path = \"sub\\\\x.rs\"]\nmod x;\n", &[])
}

pub(in crate::checks::expect_allowlist::tests) fn path_absolute() -> Repo {
    with_files("path-absolute", "#[path = \"/abs/x.rs\"]\nmod x;\n", &[])
}

pub(in crate::checks::expect_allowlist::tests) fn path_outside_repo() -> Repo {
    with_files(
        "path-outside-repo",
        "#[path = \"../../x.rs\"]\nmod x;\n",
        &[],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn path_in_inline_mod() -> Repo {
    with_files(
        "path-in-inline-mod",
        "mod outer {\n    #[path = \"c.rs\"]\n    mod c;\n}\n",
        &[("src/c.rs", "fn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn include_listed_rs() -> Repo {
    with_files(
        "include-listed-rs",
        "mod m {\n    include!(\"../gen/b.rs\");\n}\n",
        &[("gen/b.rs", "fn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn include_non_rs() -> Repo {
    with_files(
        "include-non-rs",
        "include!(\"x.inc\");\n",
        &[("src/x.inc", "#[allow(dead_code)]\nfn g() {}\n")],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn include_not_a_literal() -> Repo {
    with_files(
        "include-not-a-literal",
        "include!(concat!(env!(\"OUT_DIR\"), \"/x.rs\"));\n",
        &[],
    )
}

pub(in crate::checks::expect_allowlist::tests) fn include_in_macro_body() -> Repo {
    with_files(
        "include-in-macro-body",
        "macro_rules! i {\n    () => {\n        include!(\"x.inc\");\n    };\n}\n",
        &[],
    )
}

/// A tracked `.rs` file that is not UTF-8: it is unread, so no site is counted.
pub(in crate::checks::expect_allowlist::tests) fn rust_file_not_utf8() -> Repo {
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
pub(in crate::checks::expect_allowlist::tests) fn no_rust_file() -> Repo {
    run_files(
        "no-rust-file",
        &[(ALLOWLIST, "# expect allowlist\nsrc/a.rs f dead_code\n")],
        &[],
    )
}
