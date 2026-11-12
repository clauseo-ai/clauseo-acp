//! The planted cases of the unix-paths check: one `cases!` list, which makes `PLANTED` and one
//! test per case (see `checks/planted.rs`). Their builders are in `tests/repos.rs`. A case
//! inspects the test files it writes.

use super::flagged;
use crate::checks::planted::{fails, failure};
use crate::checks::unix_paths::Fix;

crate::checks::planted::cases! {
    /// `/opt/` in a file under `tests/` fails at its line.
    opt_in_tests_folder: Fails[flagged("crates/host/tests/a.rs:2", "/opt/homebrew/bin/npx")],
        inspects 1 => super::repos::opt_in_tests_folder,
    /// `/usr/` fails in a file under `src/` whose name holds `tests`.
    usr_in_tests_file_name: Fails[flagged(
        "crates/host/src/import_tests.rs:2",
        "/usr/local/bin/node",
    )], inspects 1 => super::repos::usr_in_tests_file_name,
    /// `/tools/` fails in a file under a `tests` folder in `src/`.
    tools_in_src_tests_folder: Fails[flagged(
        "crates/host/src/agent/tests/redact.rs:2",
        "/tools/github-mcp",
    )], inspects 1 => super::repos::tools_in_src_tests_folder,
    /// `/Users/` fails.
    users_in_tests_folder: Fails[flagged("crates/host/tests/a.rs:2", "/Users/rohan/x")],
        inspects 1 => super::repos::users_in_tests_folder,
    /// `/home/` fails.
    home_in_tests_folder: Fails[flagged("crates/host/tests/a.rs:2", "/home/runner/x")],
        inspects 1 => super::repos::home_in_tests_folder,
    /// A raw string counts.
    raw_string: Fails[flagged("crates/host/tests/a.rs:2", "/opt/raw")],
        inspects 1 => super::repos::raw_string,
    /// A path in a comment or a doc comment is no literal: it passes.
    comments_and_doc_comments: Passes, inspects 1 => super::repos::comments_and_doc_comments,
    /// `/tmp/`, `/var/` and `/etc/` are only counted, and a prefix without its `/` is no match.
    other_paths_and_prefixes: Passes, inspects 1 => super::repos::other_paths_and_prefixes,
    /// A cfg that keeps an item, a const or a statement off Windows exempts it.
    cfg_on_item: Passes, inspects 1 => super::repos::cfg_on_item,
    /// `cfg(windows)` and `cfg(any(unix, windows))` keep code on Windows: those fail.
    cfg_windows_still_fails: Fails[
        flagged("crates/host/tests/a.rs:4", "/opt/b"),
        flagged("crates/host/tests/a.rs:6", "/opt/c"),
    ], inspects 1 => super::repos::cfg_windows_still_fails,
    /// A cfg on an inline module, outer or inner, exempts what it holds.
    cfg_on_module: Passes, inspects 1 => super::repos::cfg_on_module,
    /// A `#![cfg(unix)]` at the top of a file exempts the file.
    cfg_on_whole_file: Passes, inspects 1 => super::repos::cfg_on_whole_file,
    /// A cfg on the `mod` line in the parent file exempts the module's file, through a
    /// `#[path]`, and the files under it.
    cfg_on_mod_line: Passes, inspects 4 => super::repos::cfg_on_mod_line,
    /// A `mod` line with no Unix cfg exempts nothing in its file; the Unix one exempts its own.
    mod_line_without_cfg_fails: Fails[flagged("crates/host/src/tests.rs:3", "/opt/c")],
        inspects 2 => super::repos::mod_line_without_cfg_fails,
    /// A literal inside `testing::abs(..)`, under any path, or a bare `abs(..)`, passes.
    through_abs: Passes, inspects 1 => super::repos::through_abs,
    /// Another module's `abs` is not the helper.
    through_another_abs_fails: Fails[flagged("crates/host/tests/a.rs:2", "/opt/x")],
        inspects 1 => super::repos::through_another_abs_fails,
    /// The marker in the leading comment block exempts the file.
    file_marker: Passes, inspects 1 => super::repos::file_marker,
    /// The marker on the line above, or on the line itself, exempts the line.
    line_markers: Passes, inspects 1 => super::repos::line_markers,
    /// A marker below the code exempts nothing there.
    marker_below_the_code_fails: Fails[flagged("crates/host/tests/a.rs:2", "/opt/a")],
        inspects 1 => super::repos::marker_below_the_code_fails,
    /// A marker with no reason fails, and exempts nothing.
    marker_without_reason: Fails[
        fails(
            "crates/host/tests/a.rs:2",
            "this unix-paths-ok marker gives no reason, so it exempts nothing",
            Fix::GiveReason,
        ),
        flagged("crates/host/tests/a.rs:3", "/opt/a"),
    ], inspects 1 => super::repos::marker_without_reason,
    /// The protocol's corpus and snapshots and `testing.rs` are not tests of paths: they pass,
    /// and are not counted.
    data_files: Passes, inspects 1 => super::repos::data_files,
    /// In-file `#[cfg(test)]` code of another source file is only counted.
    in_file_cfg_test_is_only_counted: Passes,
        inspects 1 => super::repos::in_file_cfg_test_is_only_counted,
    /// Two identical literals with a baseline line each pass.
    baselined: Passes, inspects 1 => super::repos::baselined,
    /// A second identical literal with only one baseline line fails, at the second.
    baseline_short_by_one: Fails[fails(
        "crates/host/tests/a.rs:2",
        "\"/opt/x\" is a Unix absolute path, which is not absolute on Windows; \
         xtask/unix-paths-baseline.txt lists it 1 time, but this file has 2",
        Fix::UseAbs,
    )], inspects 1 => super::repos::baseline_short_by_one,
    /// A baseline line whose literal is gone fails at its line.
    stale_entry: Fails[fails(
        "xtask/unix-paths-baseline.txt:3",
        "stale entry: crates/host/tests/a.rs no longer holds \"/opt/gone\"",
        Fix::Stale { line: 3 },
    )], inspects 1 => super::repos::stale_entry,
    /// A baseline line with no quoted literal fails at its line.
    malformed_entry: Fails[fails(
        "xtask/unix-paths-baseline.txt:2",
        "this line is not \"<path> \\\"<literal>\\\"\"",
        Fix::Malformed,
    )], inspects 1 => super::repos::malformed_entry,
    /// A test file the lexer refuses fails at the line it names.
    not_lexed: Fails[fails(
        "crates/host/tests/a.rs:2",
        "`crates/host/tests/a.rs` does not lex as Rust: cannot parse string into token stream",
        Fix::NotLexed,
    )], inspects 1 => super::repos::not_lexed,
    /// Outside a git repo the check says git's error, then that it stopped before it looked.
    not_a_git_repo: Fails[failure(
        "could not list the repo's files: `git ls-files -z --cached --others --exclude-standard` \
         failed: fatal: not a git repository (or any of the parent directories): .git",
        crate::checks::Fix::GitFailed {
            check: "unix-paths".to_owned(),
        },
    )], inspects 0 => super::repos::not_a_git_repo,
}
