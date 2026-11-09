//! Unit tests of the log-literals check. Its planted cases are in `tests/planted.rs`, and the
//! repos they run on are built in `tests/repos.rs`.

pub(super) mod planted;
mod repos;

use super::*;
use calls::judge;
use proc_macro2::TokenStream;

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
        assert_eq!(judge("warn", &tokens(call)), None, "{call}");
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
        assert_eq!(
            judge("error", &tokens(call)),
            Some("formats its message"),
            "{call}"
        );
    }
}

#[test]
fn an_event_at_warn_or_error_is_judged_and_a_quiet_one_passes() {
    let event = |call: &str| judge("event", &tokens(call));
    assert_eq!(
        event(r#"Level::WARN, "{name} went""#),
        Some("formats its message")
    );
    assert_eq!(
        event(r#"target: "x", tracing::Level::ERROR, "went: {}", name"#),
        Some("formats its message")
    );
    assert_eq!(event(r#"level, "{name}""#), Some("formats its message"));
    assert_eq!(event(r#"Level::INFO, "{name} went""#), None);
    assert_eq!(event(r#"Level::WARN, code = 5, "went""#), None);
}

#[test]
fn a_message_field_is_judged_in_each_form() {
    for call in [
        "message = name",
        r#"code = 5, message = "went""#,
        r#""message" = name"#,
        "%message",
        "?message",
        "message",
    ] {
        assert_eq!(
            judge("warn", &tokens(call)),
            Some("sets its message as a field"),
            "{call}"
        );
    }
    assert_eq!(judge("warn", &tokens(r#""message""#)), None);
    assert_eq!(judge("warn", &tokens(r#"messages = 2, "went""#)), None);
}

#[test]
fn tests_setup_and_other_folders_are_not_read() {
    assert!(is_read("crates/sandbox/src/windows/runner/outer.rs"));
    for file in SETUP_READ {
        assert!(is_read(file), "{file}");
    }
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
