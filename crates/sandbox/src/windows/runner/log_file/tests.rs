//! The runner's log file: what it keeps of each line, and where it writes.

use std::io;

use super::{LINE_MAX, LOG_FILE, LogFile, build, line_end};
use crate::test_support::TempDir;

/// The file's text after `log` ran with the runner's subscriber writing into `temp`.
fn logged(temp: &TempDir, log: impl FnOnce()) -> String {
    let dispatch = build(LogFile::new(temp.path(), "outer"));
    tracing::dispatcher::with_default(&dispatch, log);
    std::fs::read_to_string(temp.path().join(LOG_FILE)).unwrap_or_default()
}

#[test]
fn an_errors_text_and_any_other_string_field_are_dropped_and_numbers_kept() {
    let temp = TempDir::new("runner-log-fields");
    let error = io::Error::other(r"C:\Matters\Acme\Draft.docx is locked");
    let text = logged(&temp, || {
        tracing::warn!(
            code = 5,
            lost = 2_u64,
            panicked = true,
            cause = "the watch broke while the agent ran",
            path = "Acme",
            %error,
            "a pairing watch broke; the pairing is lost for the session"
        );
    });
    let [line] = text.lines().collect::<Vec<_>>()[..] else {
        panic!("one line: {text:?}");
    };
    assert!(line.starts_with("[outer "), "{line}");
    assert!(line.contains("WARN"), "{line}");
    for kept in [
        "a pairing watch broke; the pairing is lost for the session",
        "code=5",
        "lost=2",
        "panicked=true",
        "cause=the watch broke while the agent ran",
        "path=<dropped>",
        "error=<dropped>",
    ] {
        assert!(line.contains(kept), "{kept}: {line}");
    }
    assert!(
        !line.contains("Acme") && !line.contains("Matters"),
        "{line}"
    );
}

#[test]
fn a_message_holding_a_path_separator_is_withheld() {
    let temp = TempDir::new("runner-log-message");
    let shown = r"C:\Matters\Acme";
    let text = logged(&temp, || tracing::error!("{shown} went"));
    assert!(text.contains("<message withheld"), "{text}");
    assert!(!text.contains("Acme"), "{text}");
}

#[test]
fn only_warn_and_error_from_clauseo_targets_are_kept() {
    let temp = TempDir::new("runner-log-levels");
    let text = logged(&temp, || {
        tracing::info!("an info line");
        tracing::debug!("a debug line");
        tracing::warn!(target: "agent_client_protocol", "a dependency's warning");
        tracing::warn!("a warning");
        tracing::error!("an error");
    });
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(
        lines[0].ends_with("a warning") && lines[1].ends_with("an error"),
        "{text}"
    );
}

#[test]
fn a_long_line_is_cut_at_a_char_boundary() {
    let short = vec![b'a'; LINE_MAX];
    assert_eq!(line_end(&short), LINE_MAX);
    let mut long = vec![b'a'; LINE_MAX - 1];
    long.extend_from_slice("é".as_bytes());
    long.push(b'b');
    assert_eq!(line_end(&long), LINE_MAX - 1);
}
