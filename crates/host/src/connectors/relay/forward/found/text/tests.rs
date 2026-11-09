//! The text form: a search, a read, duplicates, the cap, foreign links, odd titles, the
//! structured shape still winning, and an error result.

use std::fmt::Write as _;

use serde_json::value::RawValue;
use serde_json::{Value, json};

use super::super::{Join, found};
use crate::connectors::FoundLink;

fn raw(value: &Value) -> Box<RawValue> {
    RawValue::from_string(value.to_string()).unwrap()
}

fn join(name: &str) -> Join {
    Join::of(name, Some(&json!({"name": name, "arguments": {}}))).unwrap()
}

/// A text-only result, the live server's shape.
fn text_result(text: &str) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": false})
}

fn links(name: &str, result: &Value) -> Vec<FoundLink> {
    found(&join(name), &raw(result)).unwrap().links
}

fn link(title: &str, url: &str) -> FoundLink {
    FoundLink {
        title: title.to_owned(),
        url: url.to_owned(),
    }
}

#[test]
fn a_text_search_names_each_document() {
    let text = "3 results for \"basic structure\".\n\n\
        1. d257876 \"Kesavananda Bharati v. State of Kerala\" (1973)\n   Supreme Court of India\n\n\
        2. d1939993 \"Minerva Mills Ltd. v. Union of India\"\n   https://data.clauseo.chat/d/1939993\n\n\
        3. d42  \"Waman Rao v. Union of India\" - https://data.clauseo.chat/d/d42.";
    assert_eq!(
        links("search_law", &text_result(text)),
        [
            link(
                "Kesavananda Bharati v. State of Kerala",
                "https://data.clauseo.chat/d/d257876"
            ),
            link(
                "Minerva Mills Ltd. v. Union of India",
                "https://data.clauseo.chat/d/1939993"
            ),
            link(
                "Waman Rao v. Union of India",
                "https://data.clauseo.chat/d/d42"
            ),
        ]
    );
}

/// A read names its document first; the documents its text cites are not sources.
#[test]
fn a_text_read_names_its_document() {
    let text = "d1939993 \"Minerva Mills Ltd. v. Union of India\"\n\nJUDGMENT\n\
        As held in d257876 \"Kesavananda Bharati v. State of Kerala\", ...";
    let result = json!({"content": [
        {"type": "resource_link", "uri": "https://data.clauseo.chat/d/d1"},
        {"type": "text", "text": text}
    ]});
    assert_eq!(
        links("read_document", &result),
        [link(
            "Minerva Mills Ltd. v. Union of India",
            "https://data.clauseo.chat/d/d1939993"
        )]
    );
}

#[test]
fn a_document_named_twice_is_one_link() {
    let text = "d7 \"First\"\nd7 \"First again\"\nhttps://data.clauseo.chat/d/7\n\
        https://data.clauseo.chat/d/d7/other\nd8 \"Second\"";
    let result = json!({"content": [
        {"type": "text", "text": text},
        {"type": "text", "text": "d8 \"Second, in another block\""}
    ]});
    assert_eq!(
        links("search_law", &result),
        [
            link("First", "https://data.clauseo.chat/d/7"),
            link("Second", "https://data.clauseo.chat/d/d8"),
        ]
    );
}

#[test]
fn a_text_search_keeps_its_first_fifty() {
    let mut text = String::new();
    for n in 0..80 {
        writeln!(text, "d{n} \"T{n}\"").unwrap();
    }
    let found = links("search_law", &text_result(&text));
    assert_eq!(found.len(), clauseo_protocol::MAX_SEARCH_RESULTS);
    assert_eq!(found[49], link("T49", "https://data.clauseo.chat/d/d49"));
}

/// An explicit link that is not https or not on `data.clauseo.chat` is not used: the link is
/// built from the id instead.
#[test]
fn a_foreign_or_plain_link_is_not_used() {
    let text = "d1 \"Plain\" http://data.clauseo.chat/d/d1\n\
        d2 \"Foreign\" https://evil.example/d/d2 https://data.clauseo.chat.evil.example/d/d2\n\
        d3 \"Shouted\" HTTPS://DATA.CLAUSEO.CHAT/d/3\n\
        d4 \"Glued\" https://data.clauseo.chat/d/4x";
    assert_eq!(
        links("search_law", &text_result(text)),
        [
            link("Plain", "https://data.clauseo.chat/d/d1"),
            link("Foreign", "https://data.clauseo.chat/d/d2"),
            link("Shouted", "HTTPS://DATA.CLAUSEO.CHAT/d/3"),
            link("Glued", "https://data.clauseo.chat/d/d4"),
        ]
    );
}

/// A title with an escaped quote keeps it; control and format characters go, as the record
/// cleans them; a long title is cut; an id with no closed title, or glued to a word, is no
/// document.
#[test]
fn odd_titles_and_malformed_entries() {
    let long = "x".repeat(400);
    let text = format!(
        "d1 \"State v. \\\"Ram\\\" Lal\"\n\
         d2 \"\u{7}Tab\tand\u{200b}zero\"\n\
         d3 \"{long}\"\n\
         d4 \"Never closed\nd5\"Close\" and d6 no title and abcd7 \"Glued\" /d8 \"Path\"\n\
         d123456789012345678901 \"Too long an id\" d \"No digits\" d9 \"\""
    );
    let found = links("search_law", &text_result(&text));
    let titles: Vec<&str> = found.iter().map(|link| link.title.as_str()).collect();
    assert_eq!(titles.len(), 5, "{titles:?}");
    assert_eq!(titles[0], "State v. \"Ram\" Lal");
    assert_eq!(titles[1], "Tab andzero");
    assert_eq!(
        titles[2].chars().count(),
        clauseo_protocol::MAX_WEB_TITLE_CHARS
    );
    assert_eq!(titles[3], "Close");
    assert_eq!(titles[4], "");
    assert_eq!(found[3].url, "https://data.clauseo.chat/d/d5");
}

#[test]
fn the_structured_lists_win_and_are_never_merged() {
    let text = "d9 \"From the text\"";
    let structured = json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": {"results": [{"title": "Structured", "url": "https://indiankanoon.org/doc/1/"}]}
    });
    assert_eq!(
        links("search_law", &structured),
        [link("Structured", "https://indiankanoon.org/doc/1/")]
    );
    let empty = json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": {"results": [{"title": "No link"}]}
    });
    assert_eq!(
        links("search_law", &empty),
        [link("From the text", "https://data.clauseo.chat/d/d9")]
    );
}

#[test]
fn an_error_result_finds_nothing_in_its_text() {
    let result = json!({"content": [{"type": "text", "text": "d1 \"T\""}], "isError": true});
    assert!(found(&join("search_law"), &raw(&result)).is_none());
    for odd in [
        json!({"content": "d1 \"T\""}),
        json!({"content": [{"type": "text", "text": 7}, {"type": "image", "text": "d1 \"T\""}]}),
        json!({"content": [{"text": "d1 \"T\""}, "d1 \"T\""]}),
    ] {
        assert_eq!(links("search_law", &odd), [], "{odd}");
    }
}

/// A text longer than the scan's budget is read to the budget, cut between characters: an
/// escape or a surrogate pair at the cut does not stop the prefix reading.
#[test]
fn a_long_text_is_read_to_its_budget() {
    let first = "d1 \"First\"\n";
    for filler in ["\u{1d11e}", "\n", "é", "a"] {
        let body = filler.repeat(super::MAX_SCAN_BYTES);
        let text = format!("{first}{body}\nd2 \"Past the budget\"");
        let found = links("search_law", &text_result(&text));
        assert_eq!(found, [link("First", "https://data.clauseo.chat/d/d1")]);
    }
    let raw = RawValue::from_string(json!("ab\u{1d11e}\ncd").to_string()).unwrap();
    for budget in 0..20 {
        assert!(super::prefix(&raw, budget).is_some(), "{budget}");
    }
}
