//! The planted cases of the expect-allowlist check: one `cases!` list, which makes `PLANTED`
//! and one test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module and `tests/cases.rs`. A case inspects the Rust files it writes.

crate::checks::planted::cases! {
    /// A Rust file whose one `#[expect]` has its line passes.
    listed_site: Passes, inspects 1 => super::listed_site,
    /// Doc comments and a string that mention `#[expect]` and `#![allow]` are not sites: it passes.
    doc_and_string_text: Passes, inspects 1 => super::doc_and_string_text,
    /// An outer `#[expect]` with no line fails at its line and names its function.
    outer: FailsWith(&[
        "src/a.rs:2: #[expect(dead_code)] in `g` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/a.rs g dead_code\" to xtask/expect-allowlist.txt (the \
         code owner approves it)",
    ]), inspects 1 => super::outer,
    /// An inner `#![expect]` at the top of the file fails, and its item is `-`.
    inner_at_file_top: FailsWith(&[
        "src/a.rs:1: #![expect(unsafe_code)] at the top of the file is not in \
         xtask/expect-allowlist.txt\n    fix: remove the #[expect], or add \"src/a.rs - \
         unsafe_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::inner_at_file_top,
    /// An inner `#![expect]` inside a function names the function.
    inner_in_fn: FailsWith(&[
        "src/a.rs:2: #![expect(unused)] in `f` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/a.rs f unused\" to xtask/expect-allowlist.txt (the \
         code owner approves it)",
    ]), inspects 1 => super::inner_in_fn,
    /// An expect under `cfg_attr` counts.
    cfg_attr: FailsWith(&[
        "src/a.rs:1: #[expect(dead_code)] in `f` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/a.rs f dead_code\" to xtask/expect-allowlist.txt (the \
         code owner approves it)",
    ]), inspects 1 => super::cfg_attr,
    /// An expect under a `cfg_attr` inside a `cfg_attr` counts, and names the `const` it is on.
    nested_cfg_attr: FailsWith(&[
        "src/a.rs:2: #[expect(dead_code)] in `C` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/a.rs C dead_code\" to xtask/expect-allowlist.txt (the \
         code owner approves it)",
    ]), inspects 1 => super::nested_cfg_attr,
    /// Each lint of one attribute needs its own line; the reason is not a lint.
    several_lints: FailsWith(&[
        "src/a.rs:1: #[expect(clippy::unwrap_used)] in `f` is not in \
         xtask/expect-allowlist.txt\n    \
         fix: remove the #[expect], or add \"src/a.rs f clippy::unwrap_used\" to \
         xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::several_lints,
    /// An expect inside a `macro_rules!` body counts, named after the macro.
    macro_rules_body: FailsWith(&[
        "src/a.rs:3: #[expect(dead_code)] in `m` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/a.rs m dead_code\" to xtask/expect-allowlist.txt (the \
         code owner approves it)",
    ]), inspects 1 => super::macro_rules_body,
    /// An expect inside the tokens of a macro call counts, named after the function around it.
    macro_call_body: FailsWith(&[
        "src/a.rs:3: #[expect(dead_code)] in `f` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/a.rs f dead_code\" to xtask/expect-allowlist.txt (the \
         code owner approves it)",
    ]), inspects 1 => super::macro_call_body,
    /// Lines with no site fail at their lines: one for a file with no such expect, one for a
    /// file that is gone.
    stale_line: FailsWith(&[
        "xtask/expect-allowlist.txt:2: stale entry: src/a.rs has no #[expect(dead_code)] in \
         `f`\n    fix: delete line 2 of xtask/expect-allowlist.txt",
        "xtask/expect-allowlist.txt:3: stale entry: src/gone.rs has no #![expect(unsafe_code)] at \
         the top of the file\n    fix: delete line 3 of xtask/expect-allowlist.txt",
    ]), inspects 1 => super::stale_line,
    /// Two identical sites (two statements in one function) need two lines: the second fails.
    two_sites_one_line: FailsWith(&[
        "src/a.rs:4: #[expect(unused)] in `f` is in xtask/expect-allowlist.txt once, but this \
         file has 2 such sites\n    fix: remove the #[expect], or add \"src/a.rs f unused\" to \
         xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::two_sites_one_line,
    /// One site listed twice: the second line is stale.
    one_site_two_lines: FailsWith(&[
        "xtask/expect-allowlist.txt:3: stale entry: \"src/a.rs f dead_code\" is listed twice, but \
         src/a.rs has 1 such site\n    fix: delete line 3 of xtask/expect-allowlist.txt",
    ]), inspects 1 => super::one_site_two_lines,
    /// An outer `#[allow]` fails.
    allow_outer: FailsWith(&[
        "src/a.rs:1: #[allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint\n    \
         fix: delete the allow, or write #[expect(dead_code, reason = \"...\")] in its place and \
         add \"src/a.rs f dead_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_outer,
    /// An inner `#![allow]` at the top of the file fails; clippy's `allow_attributes` does not
    /// see it.
    allow_inner_at_file_top: FailsWith(&[
        "src/a.rs:1: #![allow(clippy::disallowed_methods)] silences a lint; only #[expect] with a \
         reason may lower a lint\n    fix: delete the allow, or write #![expect(clippy::disallowed_methods, \
         reason = \"...\")] in its place and add \"src/a.rs - clippy::disallowed_methods\" to \
         xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_inner_at_file_top,
    /// An inner `#![allow]` at the top of an inline `mod` fails.
    allow_inner_in_mod: FailsWith(&[
        "src/a.rs:2: #![allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint\n    \
         fix: delete the allow, or write #![expect(dead_code, reason = \"...\")] in its place and \
         add \"src/a.rs m dead_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_inner_in_mod,
    /// An allow under an inner `cfg_attr` fails.
    allow_cfg_attr: FailsWith(&[
        "src/a.rs:1: #![allow(clippy::disallowed_methods)] silences a lint; only #[expect] with a \
         reason may lower a lint\n    fix: delete the allow, or write #![expect(clippy::disallowed_methods, \
         reason = \"...\")] in its place and add \"src/a.rs - clippy::disallowed_methods\" to \
         xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_cfg_attr,
    /// An allow under nested `cfg_attr` on a statement fails.
    allow_nested_cfg_attr: FailsWith(&[
        "src/a.rs:2: #[allow(unused)] silences a lint; only #[expect] with a reason may lower a lint\n    fix: \
         delete the allow, or write #[expect(unused, reason = \"...\")] in its place and add \
         \"src/a.rs f unused\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_nested_cfg_attr,
    /// An allow inside a `macro_rules!` body fails.
    allow_in_macro_rules: FailsWith(&[
        "src/a.rs:3: #![allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint\n    \
         fix: delete the allow, or write #![expect(dead_code, reason = \"...\")] in its place and \
         add \"src/a.rs m dead_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_in_macro_rules,
    /// An allow inside the tokens of a macro call fails.
    allow_in_macro_call: FailsWith(&[
        "src/a.rs:3: #[allow(dead_code)] silences a lint; only #[expect] with a reason may lower a lint\n    \
         fix: delete the allow, or write #[expect(dead_code, reason = \"...\")] in its place and \
         add \"src/a.rs f dead_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_in_macro_call,
    /// An expect under `cfg_attr` inside the tokens of a macro call counts.
    cfg_attr_in_macro_call: FailsWith(&[
        "src/a.rs:2: #[expect(dead_code)] in `f` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/a.rs f dead_code\" to xtask/expect-allowlist.txt (the \
         code owner approves it)",
    ]), inspects 1 => super::cfg_attr_in_macro_call,
    /// A bare `#[allow]` names no lint and fails.
    allow_bare_path: FailsWith(&[
        "src/a.rs:1: an allow attribute that names no lint\n    fix: delete the allow: it names \
         no lint",
    ]), inspects 1 => super::allow_bare_path,
    /// An allow that names no lint fails.
    allow_with_no_lint: FailsWith(&[
        "src/a.rs:1: an allow attribute that names no lint\n    fix: delete the allow: it names \
         no lint",
    ]), inspects 1 => super::allow_with_no_lint,
    /// A line that is not three fields fails at its line.
    malformed_line: FailsWith(&[
        "xtask/expect-allowlist.txt:2: this line has 2 fields; a line is \"<path> <item> \
         <lint>\"\n    fix: write the line as \"<path> <item> <lint>\", three fields separated by \
         spaces, as the comment at the top of xtask/expect-allowlist.txt says, or delete it",
        "xtask/expect-allowlist.txt:3: this line has 4 fields; a line is \"<path> <item> \
         <lint>\"\n    fix: write the line as \"<path> <item> <lint>\", three fields separated by \
         spaces, as the comment at the top of xtask/expect-allowlist.txt says, or delete it",
    ]), inspects 1 => super::malformed_line,
    /// A Rust file `syn` cannot parse fails at the error's line, and its line in the allowlist is
    /// not called stale.
    unparsable_file: FailsWith(&[
        "src/a.rs:2: `src/a.rs` is not valid Rust: expected one of: `for`, parentheses, \
         `unsafe`, `fn`, `extern`, identifier, `::`, `<`, `dyn`, square brackets, `*`, `&`, \
         `!`, `impl`, `_`, lifetime\n    fix: make the file valid Rust that syn can parse: \
         expect-allowlist reads every .rs file in the repo for its expect, allow and warn \
         attributes",
    ]), inspects 1 => super::unparsable_file,
    /// A missing allowlist is one failure with no line; no site is matched.
    missing_allowlist: FailsWith(&[
        "xtask/expect-allowlist.txt is missing, so no #[expect] site was matched against it\n    \
         fix: restore xtask/expect-allowlist.txt, one \"<path> <item> <lint>\" line per #[expect] \
         site; the check matches the sites against it only once it is there",
    ]), inspects 1 => super::missing_allowlist,
    /// A tracked `.rs` file that is not UTF-8 is unread: the check says why, and counts none.
    rust_file_not_utf8: FailsWith(&[
        "src/a.rs:1: could not read `src/a.rs`: stream did not contain valid UTF-8\n    \
         fix: make the file readable as UTF-8 text",
    ]), inspects 0 => super::rust_file_not_utf8,
    /// A repo with no Rust file inspects none: the stale entry is printed, and the zero line
    /// gives the normal fix beside it.
    no_rust_file: FailsWith(&[
        "xtask/expect-allowlist.txt:2: stale entry: src/a.rs has no #[expect(dead_code)] in \
         `f`\n    fix: delete line 2 of xtask/expect-allowlist.txt",
        "0 Rust files inspected in the repo, matched against xtask/expect-allowlist.txt\n    \
         fix: expect-allowlist looks for Rust files in the repo, matched against \
         xtask/expect-allowlist.txt; restore them, or, if they moved, update the consts \
         xtask/src/checks/expect_allowlist.rs builds that place from",
    ]), inspects 0 => super::no_rust_file,
    /// An untracked file that `.gitignore` does not exclude counts, and so does an upper-case
    /// `.RS`.
    untracked_upper_case_rs: FailsWith(&[
        "src/B.RS:1: #[expect(dead_code)] in `f` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/B.RS f dead_code\" to xtask/expect-allowlist.txt (the \
         code owner approves it)",
    ]), inspects 1 => super::untracked_upper_case_rs,
    /// An outer `#[warn]` fails as an allow does: it turns a denied lint into a warning.
    warn_outer: FailsWith(&[
        "src/a.rs:1: #[warn(clippy::disallowed_methods)] turns a denied lint into a warning, which a local cargo xtask check passes; only #[expect] with a reason may lower a lint\n    fix: delete the warn, or write #[expect(clippy::disallowed_methods, reason = \"...\")] in its place and add \"src/a.rs f clippy::disallowed_methods\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::warn_outer,
    /// An inner `#![warn]` at the top of the file fails.
    warn_inner_at_file_top: FailsWith(&[
        "src/a.rs:1: #![warn(warnings)] turns a denied lint into a warning, which a local cargo xtask check passes; only #[expect] with a reason may lower a lint\n    fix: delete the warn, or write #![expect(warnings, reason = \"...\")] in its place and add \"src/a.rs - warnings\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::warn_inner_at_file_top,
    /// A warn under nested `cfg_attr` on a statement fails.
    warn_cfg_attr: FailsWith(&[
        "src/a.rs:2: #[warn(unused)] turns a denied lint into a warning, which a local cargo xtask check passes; only #[expect] with a reason may lower a lint\n    fix: delete the warn, or write #[expect(unused, reason = \"...\")] in its place and add \"src/a.rs f unused\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::warn_cfg_attr,
    /// A warn inside a `macro_rules!` body fails.
    warn_in_macro_rules: FailsWith(&[
        "src/a.rs:3: #![warn(dead_code)] turns a denied lint into a warning, which a local cargo xtask check passes; only #[expect] with a reason may lower a lint\n    fix: delete the warn, or write #![expect(dead_code, reason = \"...\")] in its place and add \"src/a.rs m dead_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::warn_in_macro_rules,
    /// A warn inside the tokens of a macro call fails.
    warn_in_macro_call: FailsWith(&[
        "src/a.rs:3: #[warn(dead_code)] turns a denied lint into a warning, which a local cargo xtask check passes; only #[expect] with a reason may lower a lint\n    fix: delete the warn, or write #[expect(dead_code, reason = \"...\")] in its place and add \"src/a.rs f dead_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::warn_in_macro_call,
    /// A warn that names no lint fails.
    warn_with_no_lint: FailsWith(&[
        "src/a.rs:1: a warn attribute that names no lint\n    fix: delete the warn: it names no lint",
    ]), inspects 1 => super::warn_with_no_lint,
    /// `deny` and `forbid` only raise a lint: they pass.
    deny_and_forbid: Passes, inspects 1 => super::deny_and_forbid,
    /// `#[$m]` in a macro body fails: the caller could pass in any attribute.
    metavariable_attribute: FailsWith(&[
        "src/a.rs:3: an attribute in a macro body takes its name or a lint from a metavariable, so expect-allowlist cannot see whether it is an expect, allow or warn\n    fix: write the attribute's name and each of its lints in the macro body itself, with no metavariable, so expect-allowlist can read them; a caller cannot pass in an attribute or a lint",
    ]), inspects 1 => super::metavariable_attribute,
    /// `#[$n(dead_code)]` fails: the caller names the attribute.
    metavariable_name_with_lints: FailsWith(&[
        "src/a.rs:3: an attribute in a macro body takes its name or a lint from a metavariable, so expect-allowlist cannot see whether it is an expect, allow or warn\n    fix: write the attribute's name and each of its lints in the macro body itself, with no metavariable, so expect-allowlist can read them; a caller cannot pass in an attribute or a lint",
    ]), inspects 1 => super::metavariable_name_with_lints,
    /// `#[cfg_attr(all(), $m)]` fails.
    metavariable_in_cfg_attr: FailsWith(&[
        "src/a.rs:3: an attribute in a macro body takes its name or a lint from a metavariable, so expect-allowlist cannot see whether it is an expect, allow or warn\n    fix: write the attribute's name and each of its lints in the macro body itself, with no metavariable, so expect-allowlist can read them; a caller cannot pass in an attribute or a lint",
    ]), inspects 1 => super::metavariable_in_cfg_attr,
    /// `#![$($t)*]` fails.
    metavariable_inner_repetition: FailsWith(&[
        "src/a.rs:3: an attribute in a macro body takes its name or a lint from a metavariable, so expect-allowlist cannot see whether it is an expect, allow or warn\n    fix: write the attribute's name and each of its lints in the macro body itself, with no metavariable, so expect-allowlist can read them; a caller cannot pass in an attribute or a lint",
    ]), inspects 1 => super::metavariable_inner_repetition,
    /// `#[expect($n, ..)]` fails: one allowlist line would approve any lint.
    metavariable_lint_in_expect: FailsWith(&[
        "src/a.rs:3: an attribute in a macro body takes its name or a lint from a metavariable, so expect-allowlist cannot see whether it is an expect, allow or warn\n    fix: write the attribute's name and each of its lints in the macro body itself, with no metavariable, so expect-allowlist can read them; a caller cannot pass in an attribute or a lint",
    ]), inspects 1 => super::metavariable_lint_in_expect,
    /// A `#` with no brackets after it in a macro body fails: `# $b` takes them from the caller.
    hash_without_brackets: FailsWith(&[
        "src/a.rs:3: a `#` in a macro body has no brackets after it, so the caller supplies the attribute and expect-allowlist cannot read it\n    fix: write the attribute's brackets right after its `#`, as `#[..]` or `#![..]`, with its name and lints in the macro body itself",
    ]), inspects 1 => super::hash_without_brackets,
    /// `#[path]` to a `.rs` file git lists passes; both files are read.
    path_to_listed_rs: Passes, inspects 2 => super::path_to_listed_rs,
    /// `#[path]` to a file that is not `.rs` fails: the allow in it is never read.
    path_to_non_rs: FailsWith(&[
        "src/a.rs:1: #[path = \"x.txt\"] names `src/x.txt`, which is not a .rs file git lists, so expect-allowlist does not read it\n    fix: give #[path] a string literal naming a .rs file that git lists, relative to this file's folder, on a module at the top level of the file, or move the module to its default path; expect-allowlist reads only those files",
    ]), inspects 1 => super::path_to_non_rs,
    /// `#[path]` under `cfg_attr` to a `.rs` file `.gitignore` excludes fails.
    path_to_ignored_rs: FailsWith(&[
        "src/a.rs:1: #[path = \"gen/x.rs\"] names `src/gen/x.rs`, which is not a .rs file git lists, so expect-allowlist does not read it\n    fix: give #[path] a string literal naming a .rs file that git lists, relative to this file's folder, on a module at the top level of the file, or move the module to its default path; expect-allowlist reads only those files",
    ]), inspects 1 => super::path_to_ignored_rs,
    /// `#[path]` that leaves the repo fails.
    path_outside_repo: FailsWith(&[
        "src/a.rs:1: #[path = \"../../x.rs\"] names `../../x.rs`, outside the repo, which is not a .rs file git lists, so expect-allowlist does not read it\n    fix: give #[path] a string literal naming a .rs file that git lists, relative to this file's folder, on a module at the top level of the file, or move the module to its default path; expect-allowlist reads only those files",
    ]), inspects 1 => super::path_outside_repo,
    /// `#[path]` on a module inside an inline module fails: its folder is not checked.
    path_in_inline_mod: FailsWith(&[
        "src/a.rs:2: #[path = \"c.rs\"] is not on a module at the top level of the file, so expect-allowlist cannot tell which file rustc compiles\n    fix: give #[path] a string literal naming a .rs file that git lists, relative to this file's folder, on a module at the top level of the file, or move the module to its default path; expect-allowlist reads only those files",
    ]), inspects 2 => super::path_in_inline_mod,
    /// `include!` of a `.rs` file git lists passes, relative to the file's folder.
    include_listed_rs: Passes, inspects 2 => super::include_listed_rs,
    /// `include!` of a file that is not `.rs` fails.
    include_non_rs: FailsWith(&[
        "src/a.rs:1: include!(\"x.inc\") compiles `src/x.inc`, which is not a .rs file git lists, so expect-allowlist does not read it\n    fix: move the code into a module, or include! only a .rs file that git lists, named by a string literal relative to this file's folder; expect-allowlist reads only those files",
    ]), inspects 1 => super::include_non_rs,
    /// `include!` of anything but a string literal fails.
    include_not_a_literal: FailsWith(&[
        "src/a.rs:1: include! takes an argument that is not a string literal, so expect-allowlist cannot tell which file rustc compiles\n    fix: move the code into a module, or include! only a .rs file that git lists, named by a string literal relative to this file's folder; expect-allowlist reads only those files",
    ]), inspects 1 => super::include_not_a_literal,
    /// `include!` in a macro body counts too.
    include_in_macro_body: FailsWith(&[
        "src/a.rs:3: include!(\"x.inc\") compiles `src/x.inc`, which is not a .rs file git lists, so expect-allowlist does not read it\n    fix: move the code into a module, or include! only a .rs file that git lists, named by a string literal relative to this file's folder; expect-allowlist reads only those files",
    ]), inspects 1 => super::include_in_macro_body,
}
