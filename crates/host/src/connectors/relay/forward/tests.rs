//! The header checks, the log's method names, and the raw reply reads.

use super::reply::{MAX_ID_BYTES, checked_protocol_version, checked_session_id, one_line, same_id};
use super::*;

fn raw(text: &str) -> Box<serde_json::value::RawValue> {
    serde_json::value::RawValue::from_string(text.to_owned()).unwrap()
}

#[test]
fn protocol_versions_fit_for_a_header() {
    for good in ["2025-06-18", "2026-07-28", "v1.2_rc"] {
        assert_eq!(checked_protocol_version(good), Some(good));
    }
    let long = "1".repeat(33);
    for bad in [
        "",
        "2025-06-18\r\nX: 1",
        "2025 06",
        "2025/06/18",
        long.as_str(),
        "é",
    ] {
        assert_eq!(checked_protocol_version(bad), None, "{bad:?}");
    }
}

#[test]
fn session_ids_fit_for_a_header() {
    let longest = "~".repeat(256);
    for good in ["fake-session-0001", "!", longest.as_str()] {
        assert_eq!(checked_session_id(good), Some(good));
    }
    let long = "a".repeat(257);
    for bad in ["", "a b", "a\tb", "a\r\nb", "é", long.as_str()] {
        assert_eq!(checked_session_id(bad), None, "{bad:?}");
    }
}

#[test]
fn the_log_shows_plain_method_names_only() {
    assert_eq!(log_name(Some("tools/call")), "tools/call");
    assert_eq!(
        log_name(Some("notifications/initialized")),
        "notifications/initialized"
    );
    assert_eq!(log_name(None), "response");
    let long = "m".repeat(65);
    for bad in ["", "a b", "a\nb", "secret=1", long.as_str()] {
        assert_eq!(log_name(Some(bad)), "other", "{bad:?}");
    }
}

#[test]
fn an_answer_line_is_its_raw_bytes_on_one_line() {
    let text = "{\"id\" :1,\r\n \"result\":{\"t\":\"a\\nb\"}}";
    assert_eq!(
        one_line(&raw(text)),
        b"{\"id\" :1,   \"result\":{\"t\":\"a\\nb\"}}".to_vec()
    );
    let line = one_line(&raw(text));
    let parsed: Value = serde_json::from_slice(&line).unwrap();
    assert_eq!(parsed, serde_json::from_str::<Value>(text).unwrap());
}

#[test]
fn an_id_is_compared_only_when_small() {
    assert!(same_id(&raw("7"), &json!(7)));
    assert!(same_id(&raw(r#""a\u0062""#), &json!("ab")));
    assert!(!same_id(&raw("7"), &json!("7")));
    assert!(!same_id(&raw("null"), &json!(7)));
    let long = "x".repeat(MAX_ID_BYTES);
    assert!(!same_id(&raw(&format!("\"{long}\"")), &json!(long)));
    let fits = "x".repeat(MAX_ID_BYTES - 2);
    assert!(same_id(&raw(&format!("\"{fits}\"")), &json!(fits)));
}
