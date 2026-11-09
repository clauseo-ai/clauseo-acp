//! The builders of the log-literals planted cases. Each is a scratch git repo, since the check
//! lists the repo's files with `git ls-files`, holding the case's one violation or none.

use crate::checks::planted::{Repo, git, scratch};

/// A scratch git repo named after `case`, with each `(path, text)` of `files` written into it.
fn git_repo(case: &str, files: &[(&str, &str)]) -> Repo {
    let dir = scratch(&format!("log-literals-{case}"), files);
    git(&dir, &["init", "-q"]);
    Repo::new(dir)
}

/// A file every case can hold: literal messages only.
const CLEAN: &str = "fn f(error: std::io::Error) {\n    tracing::warn!(code = 5, %error, \
                     \"a set-aside failed\");\n    error!(\"the watch broke\");\n}\n";

pub(super) fn literal_messages() -> Repo {
    git_repo(
        "literal-messages",
        &[("crates/sandbox/src/windows/runner/outer.rs", CLEAN)],
    )
}

pub(super) fn inline_argument() -> Repo {
    let text = "fn f(name: &str) {\n    tracing::error!(\"fine\");\n    tracing::warn!(\"{name} \
                went\");\n}\n";
    git_repo(
        "inline-argument",
        &[("crates/sandbox/src/windows/runner/outer.rs", text)],
    )
}

pub(super) fn trailing_argument() -> Repo {
    let text = "fn f(name: &str) {\n    error!(code = 5, \"{} went\", name);\n}\n";
    git_repo(
        "trailing-argument",
        &[("crates/sandbox/src/windows/apply.rs", text)],
    )
}

pub(super) fn nested_call() -> Repo {
    let text =
        "fn f(name: &str) {\n    let g = || ::tracing::warn!(\"went: {}\", name);\n    g();\n}\n";
    git_repo(
        "nested-call",
        &[("crates/sandbox/src/windows/ledger.rs", text)],
    )
}

pub(super) fn skipped_files() -> Repo {
    let formatted = "fn f(name: &str) {\n    tracing::warn!(\"{name}\");\n}\n";
    git_repo(
        "skipped-files",
        &[
            ("crates/sandbox/src/windows/runner/outer.rs", CLEAN),
            ("crates/sandbox/src/windows/runner/tests.rs", formatted),
            ("crates/sandbox/src/windows/end/kill_tests.rs", formatted),
            ("crates/sandbox/src/windows/runner/tests/lost.rs", formatted),
            ("crates/sandbox/src/windows/setup/adopt.rs", formatted),
            ("crates/sandbox/src/proxy/upstream.rs", formatted),
        ],
    )
}

pub(super) fn unparsable() -> Repo {
    git_repo(
        "unparsable",
        &[(
            "crates/sandbox/src/windows/broken.rs",
            "fn f() {\n    let x = ;\n}\n",
        )],
    )
}

/// A repo whose `runner/outer.rs` is `text`.
fn outer(case: &str, text: &str) -> Repo {
    git_repo(
        case,
        &[("crates/sandbox/src/windows/runner/outer.rs", text)],
    )
}

pub(super) fn event_at_warn() -> Repo {
    let text =
        "fn f(name: &str) {\n    tracing::event!(tracing::Level::WARN, \"{name} went\");\n}\n";
    outer("event-at-warn", text)
}

pub(super) fn message_field() -> Repo {
    let text = "fn f(name: &str) {\n    tracing::error!(code = 5, message = name);\n}\n";
    outer("message-field", text)
}

pub(super) fn renamed_macro() -> Repo {
    outer("renamed-macro", "use tracing::{info, warn as say};\n")
}

pub(super) fn wrapping_macro() -> Repo {
    let text = "macro_rules! loud {\n    ($($t:tt)*) => { tracing::error!($($t)*) };\n}\n";
    outer("wrapping-macro", text)
}

pub(super) fn setup_record_files() -> Repo {
    let formatted = "fn f(name: &str) {\n    tracing::warn!(\"{name}\");\n}\n";
    git_repo(
        "setup-record-files",
        &[
            (
                "crates/sandbox/src/windows/setup/record_write.rs",
                formatted,
            ),
            ("crates/sandbox/src/windows/setup/adopt.rs", formatted),
        ],
    )
}
