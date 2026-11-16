use serde_json::json;

use super::*;

/// The serde text of decoding `value` as `T`.
fn serde_text<T: serde::de::DeserializeOwned + std::fmt::Debug>(value: Value) -> String {
    serde_json::from_value::<T>(value).unwrap_err().to_string()
}

#[test]
fn a_category_keeps_schema_words_and_drops_values() {
    let missing = serde_text::<acp::NewSessionResponse>(json!({"modes": null}));
    assert_eq!(serde_category(&missing), "missing field `sessionId`");

    let wrong = serde_text::<acp::InitializeResponse>(json!({"protocolVersion": "secret-value"}));
    let category = serde_category(&wrong);
    assert!(!category.contains("secret-value"), "{category}");
    assert!(category.starts_with("invalid type") || category.starts_with("invalid value"));

    let line = serde_json::from_str::<Value>("{\"a\": \"secret\" x}")
        .unwrap_err()
        .to_string();
    let category = serde_category(&line);
    assert!(category.ends_with("line 1 column 16"), "{category}");
    assert!(!category.contains("secret"), "{category}");
}

#[test]
fn a_quoted_value_never_survives() {
    for text in [
        "unknown variant `my-secret`, expected one of `a`, `b`",
        "invalid type: string \"/Users/rohan/Matter\", expected u16",
        "invalid value: string \"rohan@example.com\", expected a uuid at line 3 column 9",
        "missing field `has space`",
        "custom text naming /Users/rohan",
    ] {
        let category = serde_category(text);
        for value in ["my-secret", "/Users", "rohan", "has space"] {
            assert!(!category.contains(value), "{text} gave {category}");
        }
    }
    assert_eq!(
        serde_category("invalid value: string \"x\", expected a uuid at line 3 column 9"),
        "invalid value, expected a uuid line 3 column 9"
    );
    assert_eq!(serde_category("custom text naming /Users/rohan"), "other");
}

#[test]
fn data_shows_only_its_shape() {
    assert_eq!(data_shape(None), "none");
    assert_eq!(data_shape(Some(&json!("a path /Users/x"))), "string");
    assert_eq!(
        data_shape(Some(&json!({"cwd": "/Users/rohan/m", "reason": "x"}))),
        "keys [cwd, reason]"
    );
    assert_eq!(data_shape(Some(&json!([1, 2]))), "array of 2");
}

#[test]
fn only_the_sdk_decode_failure_counts_as_one() {
    let mut error = acp::Error::parse_error();
    error.data =
        Some(json!({"error": "missing field `sessionId`", "json": {}, "phase": "deserialization"}));
    assert_eq!(decode_failure(&error), Some("missing field `sessionId`"));
    error.data = Some(json!({"error": "x", "phase": "serialization"}));
    assert_eq!(decode_failure(&error), None);
    let mut agent = acp::Error::invalid_params();
    agent.data = Some(json!({"error": "x", "phase": "deserialization"}));
    assert_eq!(decode_failure(&agent), None);
}
