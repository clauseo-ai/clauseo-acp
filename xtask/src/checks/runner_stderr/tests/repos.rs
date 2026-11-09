//! The builders of the runner-stderr planted cases. Each is a scratch git repo, since the check
//! lists the repo's files with `git ls-files`, holding the case's one violation or none.

use crate::checks::planted::{Repo, git, scratch};

/// A scratch git repo named after `case`, with each `(path, text)` of `files` written into it.
fn git_repo(case: &str, files: &[(&str, &str)]) -> Repo {
    let dir = scratch(&format!("runner-stderr-{case}"), files);
    git(&dir, &["init", "-q"]);
    Repo::new(dir)
}

/// A repo whose `runner/outer.rs` is `text`.
fn outer(case: &str, text: &str) -> Repo {
    git_repo(
        case,
        &[("crates/sandbox/src/windows/runner/outer.rs", text)],
    )
}

/// A runner file that formats errors inside `Detail::new` and writes through the API.
const CLEAN: &str = "fn f(error: std::io::Error, e: std::io::Error) {\n    \
                     stderr::refuse(RunnerKind::Pipe, Detail::new(|| format!(\"{error}\")));\n    \
                     stderr::notice(RunnerKind::Pipe, Detail::new(move || e.to_string()));\n}\n";

pub(super) fn clean_runner() -> Repo {
    outer("clean-runner", CLEAN)
}

pub(super) fn skipped_files() -> Repo {
    let loud = "fn f() {\n    eprintln!(\"x\");\n    let text = format!(\"{error}\");\n}\n";
    git_repo(
        "skipped-files",
        &[
            ("crates/sandbox/src/windows/runner/outer.rs", CLEAN),
            ("crates/sandbox/src/windows/runner/stderr.rs", loud),
            ("crates/sandbox/src/windows/runner/stderr/x.rs", loud),
            ("crates/sandbox/src/windows/runner/tests.rs", loud),
            ("crates/sandbox/src/windows/runner/x_tests.rs", loud),
            ("crates/sandbox/src/windows/runner/tests/y.rs", loud),
            ("crates/sandbox/src/windows/acl.rs", loud),
        ],
    )
}

pub(super) fn inline_test_module() -> Repo {
    let text = "fn f(error: std::io::Error) -> String {\n    Detail::new(|| format!(\"{error}\"))\n}\n\
                #[cfg(test)]\nmod tests {\n    fn f(e: E) -> String {\n        format!(\"{e}\")\n    }\n}\n";
    outer("inline-test-module", text)
}

pub(super) fn cfg_not_test_module() -> Repo {
    let text = "fn f() {}\n#[cfg(not(test))]\nmod m {\n    fn g() {\n        eprintln!(\"x\");\n    }\n}\n";
    outer("cfg-not-test-module", text)
}

pub(super) fn console_macro() -> Repo {
    outer(
        "console-macro",
        "fn f() {\n    let a = 1;\n    eprintln!(\"x {a}\");\n}\n",
    )
}

pub(super) fn print_in_bin() -> Repo {
    git_repo(
        "print-in-bin",
        &[(
            "crates/sandbox/src/bin/clauseo-sandbox-run.rs",
            "fn main() {\n    print!(\"x\");\n}\n",
        )],
    )
}

pub(super) fn write_to_stderr() -> Repo {
    git_repo(
        "write-to-stderr",
        &[(
            "crates/sandbox/src/windows/runner/inner.rs",
            "fn f(stderr: &mut Vec<u8>) {\n    writeln!(stderr, \"refused\").ok();\n}\n",
        )],
    )
}

pub(super) fn stderr_call() -> Repo {
    git_repo(
        "stderr-call",
        &[(
            "crates/sandbox/src/windows/runner/inner.rs",
            "fn f() {\n    let lock = io::stderr().lock();\n}\n",
        )],
    )
}

pub(super) fn stderr_in_closure() -> Repo {
    git_repo(
        "stderr-in-closure",
        &[(
            "crates/sandbox/src/windows/runner/inner.rs",
            "fn f() {\n    let g = || std::io::stderr();\n    g();\n}\n",
        )],
    )
}

pub(super) fn inline_error_text() -> Repo {
    outer(
        "inline-error-text",
        "fn f(error: std::io::Error) {\n    let text = format!(\"{error}\");\n}\n",
    )
}

pub(super) fn positional_error_text() -> Repo {
    outer(
        "positional-error-text",
        "fn f(err: std::io::Error) {\n    let text = format!(\"x {}\", err);\n}\n",
    )
}

pub(super) fn error_to_string() -> Repo {
    outer(
        "error-to-string",
        "fn f(error: std::io::Error) {\n    let text = error.to_string();\n}\n",
    )
}

pub(super) fn to_string_in_format() -> Repo {
    outer(
        "to-string-in-format",
        "fn f(e: std::io::Error) {\n    let text = format!(\"{}\", e.to_string());\n}\n",
    )
}

pub(super) fn detail_alias() -> Repo {
    outer("detail-alias", "use super::stderr::Detail as D;\n")
}

pub(super) fn wrapping_macro() -> Repo {
    outer(
        "wrapping-macro",
        "macro_rules! say {\n    ($($t:tt)*) => { eprintln!($($t)*) };\n}\n",
    )
}

pub(super) fn unparsable() -> Repo {
    git_repo(
        "unparsable",
        &[(
            "crates/sandbox/src/windows/runner/broken.rs",
            "fn f() {\n    let x = ;\n}\n",
        )],
    )
}
