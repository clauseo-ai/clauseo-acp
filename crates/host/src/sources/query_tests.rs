//! The query a search records: `query_of`, its order of sources, the cleaning and the cap, and the
//! joining of several queries (codex-acp 2.2.2, stage B). Split out of `tests.rs` for its size.

use serde_json::json;

use super::query_of;

#[test]
fn the_query_is_raw_input_then_action_query_then_queries_joined() {
    assert_eq!(
        query_of(&json!({"query": "raw", "action": {"query": "act"}})),
        Some("raw".to_owned())
    );
    assert_eq!(
        query_of(&json!({"query": "", "action": {"query": "act"}})),
        Some("act".to_owned())
    );
    assert_eq!(
        query_of(&json!({"query": " ", "action": {"queries": ["one", "two"]}})),
        Some("one, two".to_owned())
    );
    assert_eq!(query_of(&json!({"query": "", "action": null})), None);
    assert_eq!(
        query_of(&json!({"action": {"queries": []}})),
        None,
        "empty queries are no query"
    );
}

/// The query is cleaned the way a title is: control and bidi characters go, and it is trimmed.
#[test]
fn a_query_is_cleaned_like_a_title() {
    assert_eq!(
        query_of(&json!({"query": "abc\u{202E}def\u{7}"})),
        Some("abcdef".to_owned())
    );
    assert_eq!(
        query_of(&json!({"query": "  lease\u{202A}  "})),
        Some("lease".to_owned())
    );
    // A query that is only control characters is no query.
    assert_eq!(query_of(&json!({"query": "\u{7}\u{202E}"})), None);
    // The bidi marks go too, as the URL check refuses them (decision record 0041).
    assert_eq!(
        query_of(&json!({"query": "a\u{200E}b\u{200F}c\u{061C}d"})),
        Some("abcd".to_owned())
    );
}

/// Two or more queries in `action.queries` win over `rawInput.query`, which codex-acp sends as the
/// first query with " ..." after it (live, stage B 2026-11-05): all of them are kept, joined.
#[test]
fn several_queries_win_over_a_raw_query_that_ends_in_dots() {
    let input = json!({
        "query": "q1 ...",
        "action": {"type": "search", "query": null, "queries": ["q1", "q2", "q3"]}
    });
    assert_eq!(query_of(&input), Some("q1, q2, q3".to_owned()));
}

/// One entry in `action.queries` is the query when `rawInput.query` is empty.
#[test]
fn a_single_queries_entry_is_the_query_when_the_raw_query_is_empty() {
    let input =
        json!({"query": "", "action": {"type": "search", "query": null, "queries": ["only"]}});
    assert_eq!(query_of(&input), Some("only".to_owned()));
    // And `rawInput.query`, when it has text, still comes before one entry.
    let input = json!({"query": "raw", "action": {"queries": ["only"]}});
    assert_eq!(query_of(&input), Some("raw".to_owned()));
}

/// `action.query` still applies when `action.queries` is absent.
#[test]
fn action_query_applies_when_queries_is_absent() {
    let input = json!({"query": "", "action": {"type": "search", "query": "act"}});
    assert_eq!(query_of(&input), Some("act".to_owned()));
}

/// The joined query is capped at 1,000 characters, after the join.
#[test]
fn the_joined_queries_are_capped_at_a_thousand_characters() {
    let long = "x".repeat(600);
    let input = json!({"query": "", "action": {"queries": [long.clone(), long]}});
    let query = query_of(&input).unwrap();
    assert_eq!(query.chars().count(), 1_000);
    assert!(query.starts_with(&"x".repeat(600)));
    assert_eq!(query.get(600..606), Some(", xxxx"));
}
