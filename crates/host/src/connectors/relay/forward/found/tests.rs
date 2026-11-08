//! The relay's reading of a case-law result: the fixture's lists, an error result, a list over
//! the cap, and entries of the wrong shape.

use serde_json::value::RawValue;
use serde_json::{Value, json};

use super::{Join, found};
use crate::connectors::{CaseLawTool, FoundLink};
use crate::sources::case_law::join_key;

fn raw(value: &Value) -> Box<RawValue> {
    RawValue::from_string(value.to_string()).unwrap()
}

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/../connectors/fixtures/mcp/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn join(name: &str, arguments: &Value) -> Join {
    Join::of(name, Some(&json!({"name": name, "arguments": arguments}))).unwrap()
}

#[test]
fn the_join_is_keyed_by_the_tool_and_its_arguments() {
    let args = json!({"query": "basic structure doctrine"});
    let search = join("search_law", &args);
    assert_eq!(search.key, join_key(CaseLawTool::SearchLaw, &args));
    let none = Join::of("read_document", Some(&json!({"name": "read_document"}))).unwrap();
    assert_eq!(none.key, join_key(CaseLawTool::ReadDocument, &json!({})));
    for other in [
        "citator",
        "check_citations",
        "account",
        "recharge",
        "Search_law",
    ] {
        assert!(Join::of(other, None).is_none(), "{other}");
    }
}

#[test]
fn the_fixtures_lists_are_read() {
    let search = found(
        &join("search_law", &json!({})),
        &raw(&fixture("search-law-result.json")),
    )
    .unwrap();
    assert_eq!(search.tool, CaseLawTool::SearchLaw);
    assert_eq!(
        search.links,
        [
            FoundLink {
                title: "Kesavananda Bharati Sripadagalvaru v. State of Kerala (1973)".to_owned(),
                url: "https://indiankanoon.org/doc/257876/".to_owned(),
            },
            FoundLink {
                title: "Minerva Mills Ltd. v. Union of India (1980)".to_owned(),
                url: "https://indiankanoon.org/doc/1939993/".to_owned(),
            },
        ]
    );
    let read = found(
        &join("read_document", &json!({"doc_id": "1939993"})),
        &raw(&fixture("read-document-result.json")),
    )
    .unwrap();
    assert_eq!(read.links.len(), 1);
    assert_eq!(read.links[0].url, "https://indiankanoon.org/doc/1939993/");
}

#[test]
fn an_error_result_finds_nothing() {
    let search = join("search_law", &json!({}));
    assert!(found(&search, &raw(&fixture("insufficient-credits-result.json"))).is_none());
    assert!(found(&search, &raw(&json!({"isError": "yes"}))).is_none());
    assert!(found(&search, &raw(&json!([1, 2]))).is_none());
}

/// A list over 50 keeps its first 50; entries that are not objects or have no string link are
/// skipped; `link` stands in for `url`; a title that is not a string is empty; a missing or
/// misshapen list is no links, and the call still counts.
#[test]
fn the_lists_are_read_leniently() {
    let search = join("search_law", &json!({}));
    let many: Vec<Value> = (0..80)
        .map(|n| json!({"title": format!("T{n}"), "url": format!("https://indiankanoon.org/doc/{n}/"), "snippet": [1, 2]}))
        .collect();
    let result = json!({"structuredContent": {"results": many}, "isError": false});
    let links = found(&search, &raw(&result)).unwrap().links;
    assert_eq!(links.len(), 50);
    assert_eq!(links[49].title, "T49");
    let mixed = json!({"structuredContent": {"results": [
        "a string",
        {"title": "No link"},
        {"title": "Numeric link", "url": 7},
        {"title": 12, "link": "https://indiankanoon.org/doc/1/"},
        null
    ]}});
    let links = found(&search, &raw(&mixed)).unwrap().links;
    assert_eq!(
        links,
        [FoundLink {
            title: String::new(),
            url: "https://indiankanoon.org/doc/1/".to_owned()
        }]
    );
    for result in [
        json!({"content": []}),
        json!({"structuredContent": {"results": {"not": "a list"}}}),
        json!({"structuredContent": {"documents": [{"url": "https://x.example/"}]}}),
        json!({"structuredContent": "text"}),
    ] {
        assert_eq!(found(&search, &raw(&result)).unwrap().links, [], "{result}");
    }
}

/// Bytes of title and link a `CaseLawFound` holds.
fn held(links: &[FoundLink]) -> usize {
    links
        .iter()
        .map(|link| link.title.len() + link.url.len())
        .sum()
}

/// A link a record would drop (over 2,000 characters, or no web URL) is dropped here, so 50
/// results with 16,000-character links hold almost nothing; a title is cleaned and cut as the
/// record cleans it.
#[test]
fn a_link_the_record_would_drop_is_not_kept() {
    let search = join("search_law", &json!({}));
    let long = format!("https://indiankanoon.org/doc/{}", "1".repeat(16_000 - 29));
    assert_eq!(long.len(), 16_000);
    let many: Vec<Value> = (0..50)
        .map(|_| json!({"title": "T".repeat(16_000), "url": long}))
        .collect();
    let result = json!({"structuredContent": {"results": many}});
    let links = found(&search, &raw(&result)).unwrap().links;
    assert!(held(&links) <= 1_000, "{} bytes held", held(&links));
    assert_eq!(links, []);
    let odd = json!({"structuredContent": {"results": [
        {"title": format!(" A\u{200b}\tcase {} ", "x".repeat(400)), "url": "https://a.example/1"},
        {"title": "No scheme", "url": "indiankanoon.org/doc/1/"},
        {"title": "Bidi", "url": "https://a.example/\u{202e}1"},
    ]}});
    let links = found(&search, &raw(&odd)).unwrap().links;
    assert_eq!(links.len(), 1, "{links:?}");
    assert!(
        links[0].title.starts_with("A case xxx"),
        "{}",
        links[0].title
    );
    assert_eq!(
        links[0].title.chars().count(),
        clauseo_protocol::MAX_WEB_TITLE_CHARS
    );
}

/// A link whose logged form is short but whose text is long (a 16,000-character fragment, or a
/// secret-looking query value the log redacts) is kept in its logged form, so the thread holds
/// the record's cap, not the text's.
#[test]
fn a_link_is_kept_in_its_logged_form() {
    let search = join("search_law", &json!({}));
    let fragment = format!("https://indiankanoon.org/doc/1/#{}", "a".repeat(16_000));
    let query = format!("https://indiankanoon.org/doc/1/?t={}", "a".repeat(16_000));
    // Read lists stop at 50 entries: 25 of each.
    let many: Vec<Value> = (0..50)
        .map(|n| json!({"title": "T", "url": if n % 2 == 0 { &fragment } else { &query }}))
        .collect();
    let result = json!({"structuredContent": {"results": many}});
    let links = found(&search, &raw(&result)).unwrap().links;
    assert!(held(&links) <= 130_000, "{} bytes held", held(&links));
    assert_eq!(links.len(), 50);
    assert_eq!(links[0].url, "https://indiankanoon.org/doc/1/");
    assert_eq!(links[1].url, "https://indiankanoon.org/doc/1/?t=REDACTED");
    let longest = links.iter().map(|link| link.url.len()).max().unwrap();
    assert!(longest <= 2_000, "a link of {longest} bytes");
}

/// The connector is https: an `http://` link in a case-law result is dropped, in either case,
/// while an `HTTPS://` one is kept.
#[test]
fn a_case_law_link_must_be_https() {
    let search = join("search_law", &json!({}));
    let result = json!({"structuredContent": {"results": [
        {"title": "Plain", "url": "http://example.com/a"},
        {"title": "Shouted", "url": "HTTP://example.com/b"},
        {"title": "Secure", "url": "HTTPS://indiankanoon.org/doc/2/"},
    ]}});
    let links = found(&search, &raw(&result)).unwrap().links;
    assert_eq!(
        links,
        [FoundLink {
            title: "Secure".to_owned(),
            url: "HTTPS://indiankanoon.org/doc/2/".to_owned(),
        }]
    );
}
