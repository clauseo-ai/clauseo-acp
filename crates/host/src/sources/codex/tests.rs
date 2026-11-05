//! Codex's parsing edge cases: which actions record what, and when a search is one.

use clauseo_protocol::{PageVia, ToolKind};
use serde_json::json;

use super::{found, is_search};
use crate::sources::Found;

#[test]
fn a_null_or_search_action_is_a_search_with_no_results() {
    let null = json!({"query": "", "action": null, "queries": []});
    assert_eq!(
        found(&json!({"query": "lease", "action": null})),
        Some(Found::Search {
            query: "lease".to_owned(),
            results: Vec::new(),
        })
    );
    assert_eq!(
        found(&json!({"query": "", "action": {"type": "search", "query": "fee"}})),
        Some(Found::Search {
            query: "fee".to_owned(),
            results: Vec::new(),
        })
    );
    // An empty query with a null action is no search.
    assert_eq!(found(&null), None);
}

#[test]
fn open_page_and_find_in_page_are_codex_open_page_visits_with_the_action_url() {
    let open =
        json!({"query": "", "action": {"type": "openPage", "url": "https://sci.gov.in/j?id=1#x"}});
    assert_eq!(
        found(&open),
        Some(Found::Page {
            url: "https://sci.gov.in/j?id=1#x".to_owned(),
            title: None,
            via: PageVia::CodexOpenPage,
        })
    );
    let find = json!({"query": "", "action": {"type": "findInPage", "url": "https://sci.gov.in/j", "pattern": "138"}});
    assert!(matches!(
        found(&find),
        Some(Found::Page {
            via: PageVia::CodexOpenPage,
            ..
        })
    ));
    // A page with no web URL is no page.
    assert_eq!(
        found(&json!({"query": "", "action": {"type": "openPage", "url": "about:blank"}})),
        None
    );
    assert_eq!(
        found(&json!({"query": "", "action": {"type": "openPage"}})),
        None
    );
}

#[test]
fn other_and_unknown_actions_record_nothing() {
    assert_eq!(
        found(&json!({"query": "", "action": {"type": "other"}})),
        None
    );
    assert_eq!(
        found(&json!({"query": "x", "action": {"type": "somethingNew"}})),
        None
    );
}

#[test]
fn a_search_is_a_search_kind_with_a_query_key() {
    let with_query = json!({"query": "", "action": null});
    assert!(is_search(ToolKind::Search, Some(&with_query)));
    assert!(!is_search(ToolKind::Search, Some(&json!({}))));
    assert!(!is_search(ToolKind::Search, None));
    assert!(!is_search(ToolKind::Fetch, Some(&with_query)));
}

/// D3: a Codex open page with a control character in its URL is no page.
#[test]
fn an_unsafe_codex_open_page_url_is_no_page() {
    assert_eq!(
        found(
            &json!({"query": "", "action": {"type": "openPage", "url": "https://c.example/\u{7}x"}})
        ),
        None
    );
}
