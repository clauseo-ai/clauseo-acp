//! The planted cases of the expect-allowlist check: one `cases!` list, which makes `PLANTED` and
//! one test per case (see `checks/planted.rs`). The builders of their repos are in
//! `tests/repos.rs`, and the fixes they name in `tests.rs`. A case inspects the Rust files it
//! writes.

use super::{Level, lower, unlisted};
use crate::checks::expect_allowlist::Fix;
use crate::checks::planted::{fails, failure};

crate::checks::planted::cases! {
    /// A Rust file whose one `#[expect]` has its line passes.
    listed_site: Passes, inspects 1 => super::repos::listed_site,
    /// Doc comments and a string that mention `#[expect]` and `#![allow]` are not sites: it passes.
    doc_and_string_text: Passes, inspects 1 => super::repos::doc_and_string_text,
    /// An outer `#[expect]` with no line fails at its line and names its function.
    outer: Fails[fails(
        "src/a.rs:2",
        "#[expect(dead_code)] in `g` is not in xtask/expect-allowlist.txt",
        unlisted("src/a.rs", "g", "dead_code"),
    )], inspects 1 => super::repos::outer,
    /// An inner `#![expect]` at the top of the file fails, and its item is `-`.
    inner_at_file_top: Fails[fails(
        "src/a.rs:1",
        "#![expect(unsafe_code)] at the top of the file is not in xtask/expect-allowlist.txt",
        unlisted("src/a.rs", "-", "unsafe_code"),
    )], inspects 1 => super::repos::inner_at_file_top,
    /// An inner `#![expect]` inside a function names the function.
    inner_in_fn: Fails[fails(
        "src/a.rs:2",
        "#![expect(unused)] in `f` is not in xtask/expect-allowlist.txt",
        unlisted("src/a.rs", "f", "unused"),
    )], inspects 1 => super::repos::inner_in_fn,
    /// An expect under `cfg_attr` counts.
    cfg_attr: Fails[fails(
        "src/a.rs:1",
        "#[expect(dead_code)] in `f` is not in xtask/expect-allowlist.txt",
        unlisted("src/a.rs", "f", "dead_code"),
    )], inspects 1 => super::repos::cfg_attr,
    /// An expect under a `cfg_attr` inside a `cfg_attr` counts, and names the `const` it is on.
    nested_cfg_attr: Fails[fails(
        "src/a.rs:2",
        "#[expect(dead_code)] in `C` is not in xtask/expect-allowlist.txt",
        unlisted("src/a.rs", "C", "dead_code"),
    )], inspects 1 => super::repos::nested_cfg_attr,
    /// Each lint of one attribute needs its own line; the reason is not a lint.
    several_lints: Fails[fails(
        "src/a.rs:1",
        "#[expect(clippy::unwrap_used)] in `f` is not in xtask/expect-allowlist.txt",
        unlisted("src/a.rs", "f", "clippy::unwrap_used"),
    )], inspects 1 => super::repos::several_lints,
    /// An expect inside a `macro_rules!` body counts, named after the macro.
    macro_rules_body: Fails[fails(
        "src/a.rs:3",
        "#[expect(dead_code)] in `m` is not in xtask/expect-allowlist.txt",
        unlisted("src/a.rs", "m", "dead_code"),
    )], inspects 1 => super::repos::macro_rules_body,
    /// An expect inside the tokens of a macro call counts, named after the function around it.
    macro_call_body: Fails[fails(
        "src/a.rs:3",
        "#[expect(dead_code)] in `f` is not in xtask/expect-allowlist.txt",
        unlisted("src/a.rs", "f", "dead_code"),
    )], inspects 1 => super::repos::macro_call_body,
    /// Lines with no site fail at their lines: one for a file with no such expect, one for a
    /// file that is gone.
    stale_line: Fails[fails(
        "xtask/expect-allowlist.txt:2",
        "stale entry: src/a.rs has no #[expect(dead_code)] in `f`",
        Fix::Stale { line: 2 },
    ), fails(
        "xtask/expect-allowlist.txt:3",
        "stale entry: src/gone.rs has no #![expect(unsafe_code)] at the top of the file",
        Fix::Stale { line: 3 },
    )], inspects 1 => super::repos::stale_line,
    /// Two identical sites (two statements in one function) need two lines: the second fails.
    two_sites_one_line: Fails[fails(
        "src/a.rs:4",
        "#[expect(unused)] in `f` is in xtask/expect-allowlist.txt once, but this file has 2 such \
         sites",
        unlisted("src/a.rs", "f", "unused"),
    )], inspects 1 => super::repos::two_sites_one_line,
    /// One site listed twice: the second line is stale.
    one_site_two_lines: Fails[fails(
        "xtask/expect-allowlist.txt:3",
        "stale entry: \"src/a.rs f dead_code\" is listed twice, but src/a.rs has 1 such site",
        Fix::Stale { line: 3 },
    )], inspects 1 => super::repos::one_site_two_lines,
    /// An outer `#[allow]` fails.
    allow_outer: Fails[fails(
        "src/a.rs:1",
        "#[allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint",
        lower(Level::Allow, "src/a.rs", "f", "dead_code", false),
    )], inspects 1 => super::repos::allow_outer,
    /// An inner `#![allow]` at the top of the file fails; clippy's `allow_attributes` does not
    /// see it.
    allow_inner_at_file_top: Fails[fails(
        "src/a.rs:1",
        "#![allow(clippy::disallowed_methods)] silences a lint; only #[expect] with a reason may \
         lower a lint",
        lower(Level::Allow, "src/a.rs", "-", "clippy::disallowed_methods", true),
    )], inspects 1 => super::repos::allow_inner_at_file_top,
    /// An inner `#![allow]` at the top of an inline `mod` fails.
    allow_inner_in_mod: Fails[fails(
        "src/a.rs:2",
        "#![allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint",
        lower(Level::Allow, "src/a.rs", "m", "dead_code", true),
    )], inspects 1 => super::repos::allow_inner_in_mod,
    /// An allow under an inner `cfg_attr` fails.
    allow_cfg_attr: Fails[fails(
        "src/a.rs:1",
        "#![allow(clippy::disallowed_methods)] silences a lint; only #[expect] with a reason may \
         lower a lint",
        lower(Level::Allow, "src/a.rs", "-", "clippy::disallowed_methods", true),
    )], inspects 1 => super::repos::allow_cfg_attr,
    /// An allow under nested `cfg_attr` on a statement fails.
    allow_nested_cfg_attr: Fails[fails(
        "src/a.rs:2",
        "#[allow(unused)] silences a lint; only #[expect] with a reason may lower a lint",
        lower(Level::Allow, "src/a.rs", "f", "unused", false),
    )], inspects 1 => super::repos::allow_nested_cfg_attr,
    /// An allow inside a `macro_rules!` body fails.
    allow_in_macro_rules: Fails[fails(
        "src/a.rs:3",
        "#![allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint",
        lower(Level::Allow, "src/a.rs", "m", "dead_code", true),
    )], inspects 1 => super::repos::allow_in_macro_rules,
    /// An allow inside the tokens of a macro call fails.
    allow_in_macro_call: Fails[fails(
        "src/a.rs:3",
        "#[allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint",
        lower(Level::Allow, "src/a.rs", "f", "dead_code", false),
    )], inspects 1 => super::repos::allow_in_macro_call,
    /// An expect under `cfg_attr` inside the tokens of a macro call counts.
    cfg_attr_in_macro_call: Fails[fails(
        "src/a.rs:2",
        "#[expect(dead_code)] in `f` is not in xtask/expect-allowlist.txt",
        unlisted("src/a.rs", "f", "dead_code"),
    )], inspects 1 => super::repos::cfg_attr_in_macro_call,
    /// A bare `#[allow]` names no lint and fails.
    allow_bare_path: Fails[fails(
        "src/a.rs:1",
        "an allow attribute that names no lint",
        Fix::EmptyLower { level: Level::Allow },
    )], inspects 1 => super::repos::allow_bare_path,
    /// An allow that names no lint fails.
    allow_with_no_lint: Fails[fails(
        "src/a.rs:1",
        "an allow attribute that names no lint",
        Fix::EmptyLower { level: Level::Allow },
    )], inspects 1 => super::repos::allow_with_no_lint,
    /// A line that is not three fields fails at its line.
    malformed_line: Fails[fails(
        "xtask/expect-allowlist.txt:2",
        "this line has 2 fields; a line is \"<path> <item> <lint>\"",
        Fix::Malformed,
    ), fails(
        "xtask/expect-allowlist.txt:3",
        "this line has 4 fields; a line is \"<path> <item> <lint>\"",
        Fix::Malformed,
    )], inspects 1 => super::repos::malformed_line,
    /// A Rust file `syn` cannot parse fails at the error's line, and its line in the allowlist is
    /// not called stale.
    unparsable_file: Fails[fails(
        "src/a.rs:2",
        "`src/a.rs` is not valid Rust: expected one of: `for`, parentheses, `unsafe`, `fn`, \
         `extern`, identifier, `::`, `<`, `dyn`, square brackets, `*`, `&`, `!`, `impl`, `_`, \
         lifetime",
        Fix::Unparsable,
    )], inspects 1 => super::repos::unparsable_file,
    /// A missing allowlist is one failure with no line; no site is matched.
    missing_allowlist: Fails[failure(
        "xtask/expect-allowlist.txt is missing, so no #[expect] site was matched against it",
        Fix::MissingAllowlist,
    )], inspects 1 => super::repos::missing_allowlist,
    /// A tracked `.rs` file that is not UTF-8 is unread: the check says why, and counts none.
    rust_file_not_utf8: Fails[fails(
        "src/a.rs:1",
        "could not read `src/a.rs`: stream did not contain valid UTF-8",
        crate::checks::Fix::NotUtf8,
    )], inspects 0 => super::repos::rust_file_not_utf8,
    /// A repo with no Rust file inspects none: the stale entry is printed, and the zero line
    /// gives the normal fix beside it.
    no_rust_file: Fails[fails(
        "xtask/expect-allowlist.txt:2",
        "stale entry: src/a.rs has no #[expect(dead_code)] in `f`",
        Fix::Stale { line: 2 },
    ), failure(
        "0 Rust files inspected in the repo, matched against xtask/expect-allowlist.txt",
        crate::checks::Fix::NothingInspected {
            name: "expect-allowlist",
            many: "Rust files",
            place: "in the repo, matched against xtask/expect-allowlist.txt".to_owned(),
            source: "xtask/src/checks/expect_allowlist.rs",
        },
    )], inspects 0 => super::repos::no_rust_file,
    /// An untracked file that `.gitignore` does not exclude counts, and so does an upper-case
    /// `.RS`.
    untracked_upper_case_rs: Fails[fails(
        "src/B.RS:1",
        "#[expect(dead_code)] in `f` is not in xtask/expect-allowlist.txt",
        unlisted("src/B.RS", "f", "dead_code"),
    )], inspects 1 => super::repos::untracked_upper_case_rs,
    /// An outer `#[warn]` fails as an allow does: it turns a denied lint into a warning.
    warn_outer: Fails[fails(
        "src/a.rs:1",
        "#[warn(clippy::disallowed_methods)] turns a denied lint into a warning, which a local \
         cargo xtask check passes; only #[expect] with a reason may lower a lint",
        lower(Level::Warn, "src/a.rs", "f", "clippy::disallowed_methods", false),
    )], inspects 1 => super::repos::warn_outer,
    /// An inner `#![warn]` at the top of the file fails.
    warn_inner_at_file_top: Fails[fails(
        "src/a.rs:1",
        "#![warn(warnings)] turns a denied lint into a warning, which a local cargo xtask check \
         passes; only #[expect] with a reason may lower a lint",
        lower(Level::Warn, "src/a.rs", "-", "warnings", true),
    )], inspects 1 => super::repos::warn_inner_at_file_top,
    /// A warn under nested `cfg_attr` on a statement fails.
    warn_cfg_attr: Fails[fails(
        "src/a.rs:2",
        "#[warn(unused)] turns a denied lint into a warning, which a local cargo xtask check \
         passes; only #[expect] with a reason may lower a lint",
        lower(Level::Warn, "src/a.rs", "f", "unused", false),
    )], inspects 1 => super::repos::warn_cfg_attr,
    /// A warn inside a `macro_rules!` body fails.
    warn_in_macro_rules: Fails[fails(
        "src/a.rs:3",
        "#![warn(dead_code)] turns a denied lint into a warning, which a local cargo xtask check \
         passes; only #[expect] with a reason may lower a lint",
        lower(Level::Warn, "src/a.rs", "m", "dead_code", true),
    )], inspects 1 => super::repos::warn_in_macro_rules,
    /// A warn inside the tokens of a macro call fails.
    warn_in_macro_call: Fails[fails(
        "src/a.rs:3",
        "#[warn(dead_code)] turns a denied lint into a warning, which a local cargo xtask check \
         passes; only #[expect] with a reason may lower a lint",
        lower(Level::Warn, "src/a.rs", "f", "dead_code", false),
    )], inspects 1 => super::repos::warn_in_macro_call,
    /// A warn that names no lint fails.
    warn_with_no_lint: Fails[fails(
        "src/a.rs:1",
        "a warn attribute that names no lint",
        Fix::EmptyLower { level: Level::Warn },
    )], inspects 1 => super::repos::warn_with_no_lint,
    /// `deny` and `forbid` only raise a lint: they pass.
    deny_and_forbid: Passes, inspects 1 => super::repos::deny_and_forbid,
    /// `#[$m]` in a macro body fails: the caller could pass in any attribute.
    metavariable_attribute: Fails[fails(
        "src/a.rs:3",
        "an attribute in a macro body takes its name or a lint from a metavariable, so \
         expect-allowlist cannot see whether it is an expect, allow or warn",
        Fix::MetavariableAttribute,
    )], inspects 1 => super::repos::metavariable_attribute,
    /// `#[$n(dead_code)]` fails: the caller names the attribute.
    metavariable_name_with_lints: Fails[fails(
        "src/a.rs:3",
        "an attribute in a macro body takes its name or a lint from a metavariable, so \
         expect-allowlist cannot see whether it is an expect, allow or warn",
        Fix::MetavariableAttribute,
    )], inspects 1 => super::repos::metavariable_name_with_lints,
    /// `#[cfg_attr(all(), $m)]` fails.
    metavariable_in_cfg_attr: Fails[fails(
        "src/a.rs:3",
        "an attribute in a macro body takes its name or a lint from a metavariable, so \
         expect-allowlist cannot see whether it is an expect, allow or warn",
        Fix::MetavariableAttribute,
    )], inspects 1 => super::repos::metavariable_in_cfg_attr,
    /// `#![$($t)*]` fails.
    metavariable_inner_repetition: Fails[fails(
        "src/a.rs:3",
        "an attribute in a macro body takes its name or a lint from a metavariable, so \
         expect-allowlist cannot see whether it is an expect, allow or warn",
        Fix::MetavariableAttribute,
    )], inspects 1 => super::repos::metavariable_inner_repetition,
    /// `#[expect($n, ..)]` fails: one allowlist line would approve any lint.
    metavariable_lint_in_expect: Fails[fails(
        "src/a.rs:3",
        "an attribute in a macro body takes its name or a lint from a metavariable, so \
         expect-allowlist cannot see whether it is an expect, allow or warn",
        Fix::MetavariableAttribute,
    )], inspects 1 => super::repos::metavariable_lint_in_expect,
    /// A `#` with no brackets after it in a macro body fails: `# $b` takes them from the caller.
    hash_without_brackets: Fails[fails(
        "src/a.rs:3",
        "a `#` in a macro body has no brackets after it, so the caller supplies the attribute and \
         expect-allowlist cannot read it",
        Fix::HashWithoutBrackets,
    )], inspects 1 => super::repos::hash_without_brackets,
    /// `#[path]` to a `.rs` file git lists passes; both files are read.
    path_to_listed_rs: Passes, inspects 2 => super::repos::path_to_listed_rs,
    /// `#[path]` to a file that is not `.rs` fails: the allow in it is never read.
    path_to_non_rs: Fails[fails(
        "src/a.rs:1",
        "#[path = \"x.txt\"] names `src/x.txt`, which is not a .rs file git lists, so \
         expect-allowlist does not read it",
        Fix::Path,
    )], inspects 1 => super::repos::path_to_non_rs,
    /// `#[path]` to a `.rs` file git lists, through a `.` segment, passes: `./b.rs` is `b.rs`.
    path_dot_segment: Passes, inspects 2 => super::repos::path_dot_segment,
    /// An `include!` in an attribute's value (`#[doc = include!(..)]`) is read like any other: a
    /// non-`.rs` file fails at the attribute's line.
    attribute_value_include: Fails[fails(
        "src/a.rs:1",
        "include!(\"x.inc\") compiles `src/x.inc`, which is not a .rs file git lists, so \
         expect-allowlist does not read it",
        Fix::Include,
    )], inspects 1 => super::repos::attribute_value_include,
    /// An `allow` on a statement in an attribute's value fails, and names the item the value is on.
    attribute_value_allow: Fails[fails(
        "src/a.rs:1",
        "#[allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint",
        lower(Level::Allow, "src/a.rs", "S", "dead_code", false),
    )], inspects 1 => super::repos::attribute_value_allow,
    /// An `expect` on a statement in an attribute's value passes with its allowlist line.
    attribute_value_expect_listed: Passes,
    inspects 1 => super::repos::attribute_value_expect_listed,
    /// An `include!` in an attribute's value in a macro body fails at the line of the `include`.
    attribute_value_include_in_macro: Fails[fails(
        "src/a.rs:3",
        "include!(\"x.inc\") compiles `src/x.inc`, which is not a .rs file git lists, so \
         expect-allowlist does not read it",
        Fix::Include,
    )], inspects 1 => super::repos::attribute_value_include_in_macro,
    /// An `allow` on a statement in an attribute's value in a macro body fails, and names the
    /// macro, the item the body is in.
    attribute_value_allow_in_macro: Fails[fails(
        "src/a.rs:3",
        "#[allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint",
        lower(Level::Allow, "src/a.rs", "d", "dead_code", false),
    )], inspects 1 => super::repos::attribute_value_allow_in_macro,
    /// `#[path]` to an absolute path fails: it is outside the repo, so the check does not read it.
    path_absolute: Fails[fails(
        "src/a.rs:1",
        "#[path = \"/abs/x.rs\"] names `/abs/x.rs`, outside the repo, which is not a .rs file \
         git lists, so expect-allowlist does not read it",
        Fix::Path,
    )], inspects 1 => super::repos::path_absolute,
    /// `#[path]` to a Windows drive path fails: it is outside the repo. A drive letter is no
    /// relative folder, so the check does not read it.
    path_drive_letter: Fails[fails(
        "src/a.rs:1",
        "#[path = \"C:/abs/x.rs\"] names `C:/abs/x.rs`, outside the repo, which is not a .rs file \
         git lists, so expect-allowlist does not read it",
        Fix::Path,
    )], inspects 1 => super::repos::path_drive_letter,
    /// `#[path]` under `cfg_attr` to a `.rs` file `.gitignore` excludes fails.
    path_to_ignored_rs: Fails[fails(
        "src/a.rs:1",
        "#[path = \"gen/x.rs\"] names `src/gen/x.rs`, which is not a .rs file git lists, so \
         expect-allowlist does not read it",
        Fix::Path,
    )], inspects 1 => super::repos::path_to_ignored_rs,
    /// `#[path]` that leaves the repo fails.
    path_outside_repo: Fails[fails(
        "src/a.rs:1",
        "#[path = \"../../x.rs\"] names `../../x.rs`, outside the repo, which is not a .rs file \
         git lists, so expect-allowlist does not read it",
        Fix::Path,
    )], inspects 1 => super::repos::path_outside_repo,
    /// `#[path]` on a module inside an inline module fails: its folder is not checked.
    path_in_inline_mod: Fails[fails(
        "src/a.rs:2",
        "#[path = \"c.rs\"] is not on a module at the top level of the file, so \
         expect-allowlist cannot tell which file rustc compiles",
        Fix::Path,
    )], inspects 2 => super::repos::path_in_inline_mod,
    /// `include!` of a `.rs` file git lists passes, relative to the file's folder.
    include_listed_rs: Passes, inspects 2 => super::repos::include_listed_rs,
    /// `include!` of a file that is not `.rs` fails.
    include_non_rs: Fails[fails(
        "src/a.rs:1",
        "include!(\"x.inc\") compiles `src/x.inc`, which is not a .rs file git lists, so \
         expect-allowlist does not read it",
        Fix::Include,
    )], inspects 1 => super::repos::include_non_rs,
    /// `include!` of anything but a string literal fails.
    include_not_a_literal: Fails[fails(
        "src/a.rs:1",
        "include! takes an argument that is not a string literal, so expect-allowlist cannot \
         tell which file rustc compiles",
        Fix::Include,
    )], inspects 1 => super::repos::include_not_a_literal,
    /// `include!` in a macro body counts too.
    include_in_macro_body: Fails[fails(
        "src/a.rs:3",
        "include!(\"x.inc\") compiles `src/x.inc`, which is not a .rs file git lists, so \
         expect-allowlist does not read it",
        Fix::Include,
    )], inspects 1 => super::repos::include_in_macro_body,
}
