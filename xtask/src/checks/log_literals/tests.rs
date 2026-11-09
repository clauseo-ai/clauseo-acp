//! Unit tests of the log-literals check. Its planted cases are in `tests/planted.rs`, and the
//! repos they run on are built in `tests/repos.rs`.

pub(super) mod planted;
mod repos;

use super::*;

/// The arguments of a macro call, as syn hands them over.
fn tokens(text: &str) -> TokenStream {
    text.parse().expect("tokens")
}

#[test]
fn a_literal_message_with_fields_is_not_formatted() {
    for call in [
        r#""a pairing watch broke""#,
        r#"code = 5, %error, ?reason, "a set-aside failed""#,
        r#"target: "clauseo_sandbox", "a {{braced}} word""#,
        r#"what = "a set-aside item", "a move failed","#,
        r#"error = %format!("{x}")"#,
        "",
    ] {
        assert!(!formatted(&tokens(call)), "{call}");
    }
}

#[test]
fn an_argument_in_or_after_the_message_is_formatted() {
    for call in [
        r#""{name} went""#,
        r#"code = 5, "{} went", name"#,
        r#"%error, "went: {}", reason()"#,
        r#"r"{name}""#,
    ] {
        assert!(formatted(&tokens(call)), "{call}");
    }
}

#[test]
fn tests_setup_and_other_folders_are_not_read() {
    assert!(is_read("crates/sandbox/src/windows/runner/outer.rs"));
    for file in [
        "crates/sandbox/src/windows/runner/log_file/tests.rs",
        "crates/sandbox/src/windows/runner/end/kill_tests.rs",
        "crates/sandbox/src/windows/runner/tests/lost.rs",
        "crates/sandbox/src/windows/setup/adopt.rs",
        "crates/sandbox/src/proxy/upstream.rs",
        "crates/sandbox/src/windows/notes.md",
    ] {
        assert!(!is_read(file), "{file}");
    }
}
