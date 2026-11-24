//! An agent waiting to retry its service, in both pinned adapters' shapes: codex-acp 2.10.0's
//! `Reconnecting... 1/5` (category `connection` for a dropped stream, else `service`) and
//! claude-agent-acp 0.110.1's `Retrying Claude, attempt 2 of 10.` (category `service`). Each
//! comes as a `session_info_update` whose `sessionFailure` has severity `warning`, no actions,
//! and the cause in `details`; its title and details reach the host cut to 300 characters.

use serde_json::{Value, json};

use super::{map_update, session};
use crate::event::{AcpEvent, ConnectionLost, Origin, ProtocolWarning, RETRY_TEXT_CHARS, Retrying};
use crate::map::limit::{Failure, cut, session_failure};

/// A `sessionFailure` of severity `warning` with no actions, as both adapters send a retry.
fn warning(category: &str, title: &str, details: &str) -> Value {
    json!({"jetbrains": {"air": {"version": 1, "sessionFailure": {
        "id": "019b-turn:error", "revision": 2, "category": category, "severity": "warning",
        "title": title, "details": details, "actions": [],
    }}}})
}

fn retrying(title: &str, details: &str) -> Retrying {
    Retrying {
        title: Some(title.to_owned()),
        details: Some(details.to_owned()),
    }
}

fn mapped(meta: &Value) -> Vec<AcpEvent> {
    map_update(
        json!({"sessionUpdate": "session_info_update", "_meta": meta}),
        None,
    )
}

#[test]
fn codex_at_capacity_is_a_retry_with_its_cause() {
    let meta = warning(
        "service",
        "Reconnecting... 1/5",
        "Selected model is at capacity. Please try a different model.",
    );
    let retry = retrying(
        "Reconnecting... 1/5",
        "Selected model is at capacity. Please try a different model.",
    );
    assert_eq!(
        session_failure(meta.as_object().unwrap()),
        Some(Failure::Warning(ProtocolWarning::Retrying(retry.clone())))
    );
    assert_eq!(
        mapped(&meta),
        [AcpEvent::Warning {
            session: Some(session()),
            warning: ProtocolWarning::Retrying(retry),
        }]
    );
}

#[test]
fn codex_dropped_stream_is_a_lost_connection_that_retries() {
    let details = "stream disconnected before completion: Connection reset by peer";
    let meta = warning("connection", "Reconnecting... 2/5", details);
    let lost = ConnectionLost {
        gave_up: false,
        title: Some("Reconnecting... 2/5".to_owned()),
        retry: Some(retrying("Reconnecting... 2/5", details)),
    };
    assert_eq!(
        mapped(&meta),
        [AcpEvent::ConnectionLost {
            session: session(),
            origin: Origin::Idle,
            lost,
        }]
    );
}

#[test]
fn claude_overload_is_a_retry_with_its_cause() {
    let meta = warning(
        "service",
        "Retrying Claude, attempt 2 of 10.",
        "HTTP 529 (overloaded). Next attempt in 8s.",
    );
    assert_eq!(
        mapped(&meta),
        [AcpEvent::Warning {
            session: Some(session()),
            warning: ProtocolWarning::Retrying(retrying(
                "Retrying Claude, attempt 2 of 10.",
                "HTTP 529 (overloaded). Next attempt in 8s.",
            )),
        }]
    );
}

/// A retry with no `details` (codex-acp before 2.10.0) still is one; a connection failure the
/// agent gave up on carries none.
#[test]
fn a_retry_without_details_and_a_give_up_without_retry() {
    let meta = json!({"jetbrains": {"air": {"sessionFailure": {
        "category": "service", "severity": "warning", "title": "Reconnecting... 3/5",
        "actions": [],
    }}}});
    assert_eq!(
        session_failure(meta.as_object().unwrap()),
        Some(Failure::Warning(ProtocolWarning::Retrying(Retrying {
            title: Some("Reconnecting... 3/5".to_owned()),
            details: None,
        })))
    );
    let gave_up = json!({"jetbrains": {"air": {"sessionFailure": {
        "category": "connection", "severity": "error", "title": "T", "details": "D",
        "actions": ["retry", "new_session"],
    }}}});
    assert!(matches!(
        session_failure(gave_up.as_object().unwrap()),
        Some(Failure::Connection(ConnectionLost {
            gave_up: true,
            retry: None,
            ..
        }))
    ));
}

/// Each text is cut to 300 characters on a character boundary: a multi-byte character at the
/// cut stays whole, and the uncut title of a lost connection stays as the agent sent it.
#[test]
fn retry_texts_are_cut_to_300_characters() {
    let long_title = "é".repeat(RETRY_TEXT_CHARS + 50);
    let long_details = format!("{}{}", "a".repeat(RETRY_TEXT_CHARS - 1), "😀😀😀");
    for category in ["service", "connection"] {
        let meta = warning(category, &long_title, &long_details);
        let retry = match session_failure(meta.as_object().unwrap()) {
            Some(Failure::Warning(ProtocolWarning::Retrying(retry))) => retry,
            Some(Failure::Connection(ConnectionLost {
                retry: Some(retry),
                title,
                ..
            })) => {
                assert_eq!(title.as_deref(), Some(long_title.as_str()));
                retry
            }
            other => panic!("expected a retry, got {other:?}"),
        };
        let title = retry.title.unwrap();
        let details = retry.details.unwrap();
        assert_eq!(title.chars().count(), RETRY_TEXT_CHARS, "{category}");
        assert_eq!(title, "é".repeat(RETRY_TEXT_CHARS));
        assert_eq!(details.chars().count(), RETRY_TEXT_CHARS, "{category}");
        assert!(details.ends_with("a😀"), "{category}");
    }
    assert_eq!(cut("short".to_owned()), "short");
    let exact = "x".repeat(RETRY_TEXT_CHARS);
    assert_eq!(cut(exact.clone()), exact);
}

/// A warning of another category is no retry: a limit's warning stays a plain warning.
#[test]
fn a_limit_warning_is_no_retry() {
    let meta = warning("limit", "You are close to your limit.", "80% used");
    assert!(matches!(
        session_failure(meta.as_object().unwrap()),
        Some(Failure::Warning(ProtocolWarning::SessionFailure { .. }))
    ));
}
