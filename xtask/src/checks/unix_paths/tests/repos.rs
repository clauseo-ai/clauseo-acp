//! The builders of the unix-paths planted cases. Each is a scratch git repo, since the check lists
//! the repo's files with `git ls-files`, whose files sit under `crates/host`.

use crate::checks::planted::{Repo, git, scratch};
use crate::checks::unix_paths::BASELINE;

/// The test file most cases write.
pub(super) const TEST: &str = "crates/host/tests/a.rs";

/// A scratch git repo named after `case`, with each `(path, text)` of `files` written into it.
pub(super) fn git_repo(case: &str, files: &[(&str, &str)]) -> Repo {
    let dir = scratch(&format!("unix-paths-{case}"), files);
    git(&dir, &["init", "-q"]);
    Repo::new(dir)
}

/// A repo whose one test file, `TEST`, is `text`.
fn test_file(case: &str, text: &str) -> Repo {
    git_repo(case, &[(TEST, text)])
}

/// A test function whose line 2 binds `literal`, as Rust source.
fn binds(literal: &str) -> String {
    format!("fn f() {{\n    let p = {literal};\n}}\n")
}

/// A repo whose test file, `TEST`, is `text`, and whose baseline is `entries` after one header
/// line, so they start at line 2.
fn with_baseline(case: &str, text: &str, entries: &str) -> Repo {
    let list = format!("# baseline\n{entries}");
    git_repo(case, &[(TEST, text), (BASELINE, &list)])
}

pub(super) fn opt_in_tests_folder() -> Repo {
    test_file("opt", &binds("\"/opt/homebrew/bin/npx\""))
}

pub(super) fn usr_in_tests_file_name() -> Repo {
    let text = binds("\"/usr/local/bin/node\"");
    git_repo("usr", &[("crates/host/src/import_tests.rs", &text)])
}

pub(super) fn tools_in_src_tests_folder() -> Repo {
    let text = binds("\"/tools/github-mcp\"");
    git_repo("tools", &[("crates/host/src/agent/tests/redact.rs", &text)])
}

pub(super) fn users_in_tests_folder() -> Repo {
    test_file("users", &binds("\"/Users/rohan/x\""))
}

pub(super) fn home_in_tests_folder() -> Repo {
    test_file("home", &binds("\"/home/runner/x\""))
}

pub(super) fn raw_string() -> Repo {
    test_file("raw", &binds("r#\"/opt/raw\"#"))
}

pub(super) fn comments_and_doc_comments() -> Repo {
    let text = "//! Reads \"/opt/a\".\n/// Writes \"/usr/b\".\n// \"/home/c\"\n\
                /* \"/tools/d\" */\nfn f() {}\n";
    test_file("comments", text)
}

pub(super) fn other_paths_and_prefixes() -> Repo {
    let text =
        binds("[\"/tmp/x\", \"/var/y\", \"/etc/z\", \"/options\", \"opt/x\", \"C:\\\\opt\"]");
    test_file("other-prefixes", &text)
}

pub(super) fn cfg_on_item() -> Repo {
    let text = "#[cfg(unix)]\n#[test]\nfn a() { let p = \"/opt/a\"; }\n\
                #[cfg(all(test, unix))]\nfn b() { let p = \"/opt/b\"; }\n\
                #[cfg(not(windows))]\nconst C: &str = \"/opt/c\";\n\
                #[cfg(target_os = \"macos\")]\nfn d() { let p = \"/opt/d\"; }\n\
                #[cfg(any(target_os = \"linux\", target_os = \"macos\"))]\nfn e() { let p = \"/opt/e\"; }\n\
                fn g() {\n    #[cfg(unix)]\n    let p = \"/opt/g\";\n}\n";
    test_file("cfg-on-item", text)
}

pub(super) fn cfg_windows_still_fails() -> Repo {
    let text = "#[cfg(unix)]\nfn a() {}\n#[cfg(windows)]\nfn b() { let p = \"/opt/b\"; }\n\
                #[cfg(any(unix, windows))]\nfn c() { let p = \"/opt/c\"; }\n";
    test_file("cfg-windows", text)
}

pub(super) fn cfg_on_module() -> Repo {
    let text = "#[cfg(unix)]\nmod unix {\n    fn a() { let p = \"/opt/a\"; }\n}\n\
                mod jail {\n    #![cfg(target_os = \"macos\")]\n    fn b() { let p = \"/opt/b\"; }\n}\n";
    test_file("cfg-on-module", text)
}

pub(super) fn cfg_on_whole_file() -> Repo {
    test_file(
        "cfg-on-file",
        "#![cfg(unix)]\n//! Unix only.\nfn a() { let p = \"/opt/a\"; }\n",
    )
}

pub(super) fn cfg_on_mod_line() -> Repo {
    git_repo(
        "cfg-on-mod-line",
        &[
            (
                "crates/host/tests/root.rs",
                "#[cfg(unix)]\n#[path = \"root/jail.rs\"]\nmod jail;\n",
            ),
            (
                "crates/host/tests/root/jail.rs",
                "mod inner;\nfn a() { let p = \"/opt/a\"; }\n",
            ),
            (
                "crates/host/tests/root/jail/inner.rs",
                "fn b() { let p = \"/opt/b\"; }\n",
            ),
            (
                "crates/host/src/lib.rs",
                "#[cfg(all(test, unix))]\nmod unix_tests;\n",
            ),
            (
                "crates/host/src/unix_tests.rs",
                "fn c() { let p = \"/opt/c\"; }\n",
            ),
        ],
    )
}

pub(super) fn mod_line_without_cfg_fails() -> Repo {
    git_repo(
        "mod-line-without-cfg",
        &[
            ("crates/host/src/lib.rs", "#[cfg(test)]\nmod tests;\n"),
            (
                "crates/host/src/tests.rs",
                "#[cfg(unix)]\nmod unix;\nfn c() { let p = \"/opt/c\"; }\n",
            ),
            (
                "crates/host/src/tests/unix.rs",
                "fn d() { let p = \"/opt/d\"; }\n",
            ),
        ],
    )
}

pub(super) fn through_abs() -> Repo {
    let text = "fn f() {\n    let a = crate::testing::abs(\"/tools/a\");\n    \
                let b = testing::abs(&format!(\"/opt/{}\", 1));\n    let c = abs(\"/usr/c\");\n}\n";
    test_file("abs", text)
}

pub(super) fn through_another_abs_fails() -> Repo {
    test_file("other-abs", &binds("paths::abs(\"/opt/x\")"))
}

pub(super) fn file_marker() -> Repo {
    let text = format!(
        "//! Tests of the parser.\n// unix-paths-ok: parsed as text only\n\n{}",
        binds("\"/opt/x\"")
    );
    test_file("file-marker", &text)
}

pub(super) fn line_markers() -> Repo {
    let text = "fn f() {\n    // unix-paths-ok: compared as text\n    let a = \"/opt/a\";\n    \
                let b = \"/opt/b\"; // unix-paths-ok: compared as text\n}\n";
    test_file("line-markers", text)
}

pub(super) fn marker_below_the_code_fails() -> Repo {
    let text = "fn f() {\n    let a = \"/opt/a\";\n\n    // unix-paths-ok: too far\n}\n";
    test_file("marker-below", text)
}

pub(super) fn marker_without_reason() -> Repo {
    let text = "fn f() {\n    // unix-paths-ok:\n    let a = \"/opt/a\";\n}\n";
    test_file("bare-marker", text)
}

pub(super) fn data_files() -> Repo {
    let opt = binds("\"/opt/x\"");
    git_repo(
        "data-files",
        &[
            ("crates/protocol/tests/corpus/writer.rs", &opt),
            ("crates/protocol/tests/snapshots/mcp.rs", &opt),
            ("crates/host/src/testing.rs", &opt),
            (TEST, "fn f() {}\n"),
        ],
    )
}

pub(super) fn in_file_cfg_test_is_only_counted() -> Repo {
    git_repo(
        "in-file-cfg-test",
        &[
            (
                "crates/host/src/import.rs",
                "#[cfg(test)]\nmod tests {\n    const P: &str = \"/opt/x\";\n}\n",
            ),
            (TEST, "fn f() {}\n"),
        ],
    )
}

pub(super) fn baselined() -> Repo {
    let entry = "crates/host/tests/a.rs \"/opt/x\"\n";
    with_baseline(
        "baselined",
        &binds("[\"/opt/x\", \"/opt/x\"]"),
        &entry.repeat(2),
    )
}

pub(super) fn baseline_short_by_one() -> Repo {
    let entry = "crates/host/tests/a.rs \"/opt/x\"\n";
    with_baseline("baseline-short", &binds("[\"/opt/x\", \"/opt/x\"]"), entry)
}

pub(super) fn stale_entry() -> Repo {
    let entries = "crates/host/tests/a.rs \"/opt/x\"\ncrates/host/tests/a.rs \"/opt/gone\"\n";
    with_baseline("stale", &binds("\"/opt/x\""), entries)
}

pub(super) fn malformed_entry() -> Repo {
    with_baseline(
        "malformed",
        "fn f() {}\n",
        "crates/host/tests/a.rs /opt/x\n",
    )
}

pub(super) fn not_lexed() -> Repo {
    test_file("not-lexed", "fn f() {\n    let a = \"/opt/a;\n")
}

/// Not a git checkout: the check's listing fails, so the case has no git init.
pub(super) fn not_a_git_repo() -> Repo {
    Repo::new(scratch(
        "unix-paths-not-a-git-repo",
        &[(TEST, "fn f() {}\n")],
    ))
}
