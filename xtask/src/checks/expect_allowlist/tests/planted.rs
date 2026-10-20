//! The planted cases of the expect-allowlist check: one `cases!` list, which makes `PLANTED`
//! and one test per case (see `checks/planted.rs`). The functions that build each input live in
//! the check's tests module. Every case has one Rust file, so each inspects 1.

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
    /// An expect inside a `macro_rules!` body counts, named after the module around the macro.
    macro_rules_body: FailsWith(&[
        "src/a.rs:3: #[expect(dead_code)] in `a` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/a.rs a dead_code\" to xtask/expect-allowlist.txt (the \
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
    /// Two identical sites need two lines: the second site fails.
    two_sites_one_line: FailsWith(&[
        "src/a.rs:4: #[expect(dead_code)] in `a` is in xtask/expect-allowlist.txt once, but this \
         file has 2 such sites\n    fix: remove the #[expect], or add \"src/a.rs a dead_code\" to \
         xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::two_sites_one_line,
    /// One site listed twice: the second line is stale.
    one_site_two_lines: FailsWith(&[
        "xtask/expect-allowlist.txt:3: stale entry: \"src/a.rs f dead_code\" is listed twice, but \
         src/a.rs has 1 such site\n    fix: delete line 3 of xtask/expect-allowlist.txt",
    ]), inspects 1 => super::one_site_two_lines,
    /// An outer `#[allow]` fails.
    allow_outer: FailsWith(&[
        "src/a.rs:1: #[allow(dead_code)] silences a lint; only #[expect] with a reason may\n    \
         fix: delete the allow, or write #[expect(dead_code, reason = \"...\")] in its place and \
         add \"src/a.rs f dead_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_outer,
    /// An inner `#![allow]` at the top of the file fails; clippy's `allow_attributes` does not
    /// see it.
    allow_inner_at_file_top: FailsWith(&[
        "src/a.rs:1: #![allow(clippy::disallowed_methods)] silences a lint; only #[expect] with a \
         reason may\n    fix: delete the allow, or write #![expect(clippy::disallowed_methods, \
         reason = \"...\")] in its place and add \"src/a.rs - clippy::disallowed_methods\" to \
         xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_inner_at_file_top,
    /// An inner `#![allow]` at the top of an inline `mod` fails.
    allow_inner_in_mod: FailsWith(&[
        "src/a.rs:2: #![allow(dead_code)] silences a lint; only #[expect] with a reason may\n    \
         fix: delete the allow, or write #![expect(dead_code, reason = \"...\")] in its place and \
         add \"src/a.rs m dead_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_inner_in_mod,
    /// An allow under an inner `cfg_attr` fails.
    allow_cfg_attr: FailsWith(&[
        "src/a.rs:1: #![allow(clippy::disallowed_methods)] silences a lint; only #[expect] with a \
         reason may\n    fix: delete the allow, or write #![expect(clippy::disallowed_methods, \
         reason = \"...\")] in its place and add \"src/a.rs - clippy::disallowed_methods\" to \
         xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_cfg_attr,
    /// An allow under nested `cfg_attr` on a statement fails.
    allow_nested_cfg_attr: FailsWith(&[
        "src/a.rs:2: #[allow(unused)] silences a lint; only #[expect] with a reason may\n    fix: \
         delete the allow, or write #[expect(unused, reason = \"...\")] in its place and add \
         \"src/a.rs f unused\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_nested_cfg_attr,
    /// An allow inside a `macro_rules!` body fails.
    allow_in_macro_rules: FailsWith(&[
        "src/a.rs:3: #![allow(dead_code)] silences a lint; only #[expect] with a reason may\n    \
         fix: delete the allow, or write #![expect(dead_code, reason = \"...\")] in its place and \
         add \"src/a.rs a dead_code\" to xtask/expect-allowlist.txt (the code owner approves it)",
    ]), inspects 1 => super::allow_in_macro_rules,
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
         `unsafe`, `fn`, \
         `extern`, identifier, `::`, `<`, `dyn`, square brackets, `*`, `&`, `!`, `impl`, `_`, \
         lifetime\n    fix: make the file valid Rust that syn can parse: expect-allowlist \
         reads every .rs file in the repo for its #[expect] and #[allow] sites",
    ]), inspects 1 => super::unparsable_file,
    /// A missing allowlist is one failure with no line; no site is matched.
    missing_allowlist: FailsWith(&[
        "xtask/expect-allowlist.txt is missing, so no #[expect] site was matched against it\n    \
         fix: restore xtask/expect-allowlist.txt, one \"<path> <item> <lint>\" line per #[expect] \
         site; the check matches the sites against it only once it is there",
    ]), inspects 1 => super::missing_allowlist,
    /// An untracked file that `.gitignore` does not exclude counts, and so does an upper-case
    /// `.RS`.
    untracked_upper_case_rs: FailsWith(&[
        "src/B.RS:1: #[expect(dead_code)] in `f` is not in xtask/expect-allowlist.txt\n    fix: \
         remove the #[expect], or add \"src/B.RS f dead_code\" to xtask/expect-allowlist.txt (the \
         code owner approves it)",
    ]), inspects 1 => super::untracked_upper_case_rs,
}
