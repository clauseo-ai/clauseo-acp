//! The texts by connector name.

use serde_json::Value;

use super::*;

fn message(line: &str) -> String {
    let value: Value = serde_json::from_str(line).unwrap();
    value["error"]["message"].as_str().unwrap().to_owned()
}

#[test]
fn case_law_keeps_its_texts_and_the_version_history_has_its_own() {
    let id = Value::from(1);
    let case_law = Subject::of("case-law");
    assert_eq!(
        message(&busy_for(case_law, &id)),
        "Case law is busy: too many requests are waiting for Clauseo's relay. Wait for the answers to them, then try again."
    );
    let versions = Subject::of("clauseo-versions");
    assert_eq!(
        message(&busy_for(versions, &id)),
        "The version history is busy: too many requests are waiting for Clauseo's relay. Wait for the answers to them, then try again."
    );
    assert_eq!(
        message(&unreachable_for(versions, &id, "closed")),
        "The version history is not available in this conversation: Clauseo's relay is not reachable. Tell the lawyer to start a new conversation."
    );
}

/// A name the mode does not know is never put into a text.
#[test]
fn another_name_gets_texts_that_name_nothing() {
    let other = Subject::of("other-tool");
    let id = Value::from(1);
    for line in [
        unreachable_for(other, &id, "closed"),
        busy_for(other, &id),
        line_too_long(other),
    ] {
        let text = message(&line);
        assert!(!text.contains("other-tool"), "{text}");
        assert!(!text.contains("Case law"), "{text}");
    }
    assert_eq!(
        message(&line_too_long(other)),
        "Line too long for Clauseo's relay."
    );
}
