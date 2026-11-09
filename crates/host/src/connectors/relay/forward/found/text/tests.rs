//! The text form: the live server's replies (golden), its JSON form, duplicates, the cap,
//! citations and URLs that are not sources, odd titles, the structured shape still winning, an
//! error result and the scan's budget.

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

/// A golden fixture's links, as (title, link) pairs.
fn golden(name: &str, fixture: &str) -> Vec<(String, String)> {
    let path = format!(
        "{}/../connectors/fixtures/mcp/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(path).unwrap();
    let result = RawValue::from_string(text.trim_end().to_owned()).unwrap();
    found(&join(name), &result)
        .unwrap()
        .links
        .into_iter()
        .map(|link| (link.title, link.url))
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(title, id)| {
            (
                (*title).to_owned(),
                format!("https://data.clauseo.chat/d/{id}"),
            )
        })
        .collect()
}

fn link(title: &str, id: &str) -> FoundLink {
    FoundLink {
        title: title.to_owned(),
        url: format!("https://data.clauseo.chat/d/{id}"),
    }
}

/// The ten hits of sample 1, in order, as the server titles them.
const SEARCH_HITS: [(&str, &str); 10] = [
    ("Tofan Singh v. The State Of Tamil Nadu", "d143202244"),
    (
        "Justice K.S.Puttaswamy(Retd) And Anr. v. Union Of India And Ors.",
        "d91938676",
    ),
    ("Govind v. State Of Madhya Pradesh & Anr", "d436241"),
    (
        "People'S Union For Civilliberties ... v. The Union Of India And Another",
        "d973841",
    ),
    ("R. Rajagopal v. State Of T.N", "d501107"),
    (
        "Distt. Registrar & Collector, ... v. Canara Bank Etc",
        "d1068532",
    ),
    (
        "People'S Union Of Civil Liberties ... v. Union Of India (Uoi) And Anr.",
        "d31276692",
    ),
    ("Mr. 'X' v. Hospital 'Z'", "d382721"),
    ("Gobind v. State Of Madhya Pradesh And Anr.", "d845196"),
    ("Manohar Lal Sharma v. Union Of India", "d39021018"),
];

#[test]
fn golden_search_names_its_ten_hits() {
    assert_eq!(
        golden("search_law", "ik-search-law.json"),
        pairs(&SEARCH_HITS)
    );
}

#[test]
fn golden_search_of_two_queries_names_its_eleven_hits() {
    let mut expected = vec![("Narcotics Control Bureau v. Mohit Agarwal", "d68361150")];
    expected.extend(SEARCH_HITS);
    assert_eq!(
        golden("search_law", "ik-search-law-two-queries.json"),
        pairs(&expected)
    );
}

#[test]
fn golden_read_names_its_document() {
    assert_eq!(
        golden("read_document", "ik-read-document.json"),
        pairs(&[("Tofan Singh v. The State Of Tamil Nadu", "d143202244")])
    );
}

#[test]
fn golden_read_of_two_documents_names_both() {
    assert_eq!(
        golden("read_document", "ik-read-document-two.json"),
        pairs(&[
            ("Tofan Singh v. The State Of Tamil Nadu", "d143202244"),
            ("Narcotics Control Bureau v. Mohit Agarwal", "d68361150"),
        ])
    );
}

#[test]
fn golden_search_with_no_hits_names_nothing() {
    assert_eq!(golden("search_law", "ik-search-law-no-hits.json"), []);
}

/// Only a line that starts `d<digits> "` names a document: a snippet's, a fenced paragraph's,
/// an indented or mid-line id is a citation, and a URL in the text or a `resource_link` block is
/// never a source.
#[test]
fn citations_and_urls_are_not_sources() {
    let text = "d1 \"Read\" · SC · 2020\n\
        ```\n[p3] d2 \"Cited in a paragraph\" · SC\n```\n\
          \x20\x20\"as held in d3 \"Cited in a snippet\" · SC\"\n\
         \x20d4 \"Indented\" · SC\n\
        See d5 \"Mid-line\" · SC\n\
        Saved: https://data.clauseo.chat/d/d6 and https://indiankanoon.org/doc/7/\n\
        Links: https://data.clauseo.chat/d/{id}";
    let result = json!({"content": [
        {"type": "text", "text": text},
        {"type": "resource_link", "uri": "https://data.clauseo.chat/d/d8", "name": "d8 \"File\" · x"}
    ]});
    assert_eq!(links("read_document", &result), [link("Read", "d1")]);
}

#[test]
fn a_document_named_twice_is_one_link() {
    let result = json!({"content": [
        {"type": "text", "text": "d7 \"First\" · SC\nd7 \"First again\" · SC\nd8 \"Second\" · SC"},
        {"type": "text", "text": "d8 \"Second, in another block\" · SC"}
    ]});
    assert_eq!(
        links("search_law", &result),
        [link("First", "d7"), link("Second", "d8")]
    );
}

#[test]
fn a_text_search_keeps_its_first_fifty() {
    let mut text = String::new();
    for n in 0..80 {
        writeln!(text, "d{n} \"T{n}\" · SC 2020\n  \"snippet\"").unwrap();
    }
    let found = links("search_law", &text_result(&text));
    assert_eq!(found.len(), clauseo_protocol::MAX_SEARCH_RESULTS);
    assert_eq!(found[49], link("T49", "d49"));
}

/// A title runs to the `"` before the first ` · `, or with none to the line's last `"`, so it
/// keeps its own quotes; control and format characters go, as the record cleans them; a long
/// title is cut; a line with no closed title, no space before the quote or a malformed id names
/// nothing.
#[test]
fn titles_and_malformed_lines() {
    let long = "x".repeat(400);
    let text = format!(
        "d1 \"State v. \"Ram\" · Lal\" · SC · q1\n\
         d2 \"\u{7}Tab\tand\u{200b}zero\"\n\
         d3 \"{long}\" · SC\n\
         d4 \"No close · SC\n\
         d5\"No space\" · SC\n\
         d \"No digits\" · SC\n\
         d123456789012345678901 \"Too long an id\" · SC\n\
         dx1 \"Not an id\" · SC\n\
         d9 \"\" · SC"
    );
    let found = links("search_law", &text_result(&text));
    let titles: Vec<&str> = found.iter().map(|link| link.title.as_str()).collect();
    assert_eq!(titles.len(), 4, "{titles:?}");
    assert_eq!(titles[0], "State v. \"Ram");
    assert_eq!(titles[1], "Tab andzero");
    assert_eq!(
        titles[2].chars().count(),
        clauseo_protocol::MAX_WEB_TITLE_CHARS
    );
    assert_eq!(titles[3], "");
    assert_eq!(found[3].url, "https://data.clauseo.chat/d/d9");
}

/// A JSON-mode search: the first block's `hits[]`; a document link is used, any other link is
/// rebuilt from the id; a hit with neither, or not an object, is skipped; the second block (the
/// charge lines) is not read.
#[test]
fn a_json_search_reads_its_hits() {
    let hits = json!({"hits": [
        {"id": "d1", "title": "Linked", "link": "https://data.clauseo.chat/d/d1"},
        {"id": "d2", "title": "Foreign", "link": "https://indiankanoon.org/doc/2/"},
        {"id": "d3", "title": "Plain", "link": "http://data.clauseo.chat/d/d3"},
        {"id": "d4", "title": "Elsewhere", "link": "https://data.clauseo.chat/x/d4"},
        {"id": "4", "title": "Bad id, no link"},
        {"title": "No id"},
        {"id": "d1", "title": "Again", "link": "https://data.clauseo.chat/d/d1"},
        {"id": "d5", "title": 7},
        "a string"
    ], "next_cursor": null});
    let result = json!({"content": [
        {"type": "text", "text": hits.to_string()},
        {"type": "text", "text": "d9 \"Charge block\" · x"}
    ]});
    assert_eq!(
        links("search_law", &result),
        [
            link("Linked", "d1"),
            link("Foreign", "d2"),
            link("Plain", "d3"),
            link("Elsewhere", "d4"),
            link("", "d5"),
        ]
    );
    let many: Vec<Value> = (0..80)
        .map(|n| json!({"id": format!("d{n}"), "title": format!("T{n}")}))
        .collect();
    let result = text_result(&json!({ "hits": many }).to_string());
    assert_eq!(links("search_law", &result).len(), 50);
}

/// A JSON-mode read: the first block's `documents[]`; `source_url` and `unresolved[]` are
/// never sources, a metadata-only entry counts, and a pending one is skipped.
#[test]
fn a_json_read_reads_its_documents() {
    let documents = json!({
        "documents": [
            {"id": "d143202244", "title": "Tofan Singh", "link": "https://data.clauseo.chat/d/d143202244",
             "source_url": "https://indiankanoon.org/doc/143202244/", "text": "d1 \"Cited\" · SC"},
            {"id": "d68361150", "title": "Mohit Agarwal", "link": "https://data.clauseo.chat/d/d68361150",
             "details": {"court": "SC"}},
            {"id": "d5", "pending": true},
            {"source_url": "https://indiankanoon.org/doc/6/", "title": "Source only"}
        ],
        "unresolved": [{"id": "d7", "link": "https://data.clauseo.chat/d/d7"}, "d8"],
        "next_cursor": "c"
    });
    let result = text_result(&documents.to_string());
    assert_eq!(
        links("read_document", &result),
        [
            link("Tofan Singh", "d143202244"),
            link("Mohit Agarwal", "d68361150")
        ]
    );
    // The list is the tool's: a read's `hits`, or a search's `documents`, is not read, and the
    // object without its list leaves the rest to the line form.
    let other = json!({"content": [
        {"type": "text", "text": json!({"hits": [{"id": "d1", "title": "Hit"}]}).to_string()},
        {"type": "text", "text": "d2 \"Line\" · SC"}
    ]});
    assert_eq!(links("read_document", &other), [link("Line", "d2")]);
    assert_eq!(links("search_law", &result), []);
}

#[test]
fn the_structured_lists_win_and_are_never_merged() {
    let text = "d9 \"From the text\" · SC";
    let structured = json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": {"results": [{"title": "Structured", "url": "https://indiankanoon.org/doc/1/"}]}
    });
    assert_eq!(
        links("search_law", &structured),
        [FoundLink {
            title: "Structured".to_owned(),
            url: "https://indiankanoon.org/doc/1/".to_owned()
        }]
    );
    let empty = json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": {"results": [{"title": "No link"}]}
    });
    assert_eq!(links("search_law", &empty), [link("From the text", "d9")]);
}

#[test]
fn an_error_result_finds_nothing_in_its_text() {
    let result = json!({"content": [{"type": "text", "text": "d1 \"T\" · SC"}], "isError": true});
    assert!(found(&join("search_law"), &raw(&result)).is_none());
    for odd in [
        json!({"content": "d1 \"T\" · SC"}),
        json!({"content": [{"type": "text", "text": 7}, {"type": "image", "text": "d1 \"T\" · SC"}]}),
        json!({"content": [{"text": "d1 \"T\" · SC"}, "d1 \"T\" · SC"]}),
    ] {
        assert_eq!(links("search_law", &odd), [], "{odd}");
    }
}

/// A text longer than the scan's budget is read to the budget, cut between characters: an
/// escape or a surrogate pair at the cut does not stop the prefix reading.
#[test]
fn a_long_text_is_read_to_its_budget() {
    let first = "d1 \"First\" · SC\n";
    for filler in ["\u{1d11e}", "\n", "é", "a"] {
        let body = filler.repeat(super::MAX_SCAN_BYTES);
        let text = format!("{first}{body}\nd2 \"Past the budget\" · SC");
        let found = links("search_law", &text_result(&text));
        assert_eq!(found, [link("First", "d1")]);
    }
    let raw = RawValue::from_string(json!("ab\u{1d11e}\ncd").to_string()).unwrap();
    for budget in 0..20 {
        assert!(super::prefix(&raw, budget).is_some(), "{budget}");
    }
}
