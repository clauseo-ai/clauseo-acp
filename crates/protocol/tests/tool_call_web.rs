//! A tool call's web in the view (0041): left out of the JSON when there is none, round-tripped
//! when there is, and a page kind this version does not know still decodes.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

#[cfg(test)]
#[path = "support/examples.rs"]
pub mod examples;

#[cfg(test)]
#[path = "support/entries.rs"]
pub mod entries;

use clauseo_protocol::{
    PageVia, ToolCallEntry, ToolCallPage, ToolCallSearch, ToolCallWeb, ViewEntry, WebResult,
};

use fixtures::round_trips;

/// The tool call of `entries::every_entry`, with no web records.
#[cfg(test)]
fn tool_call() -> ToolCallEntry {
    let call = entries::every_entry()
        .into_iter()
        .find_map(|entry| match entry {
            ViewEntry::ToolCall(call) => Some(*call),
            _ => None,
        })
        .unwrap();
    ToolCallEntry { web: None, ..call }
}

#[test]
fn a_tool_call_without_web_writes_no_field_and_reads_back_none() {
    // A call with no web records is written as before 0041's fields: no `web` key, and it reads
    // back `None`.
    let call = tool_call();
    let json = serde_json::to_value(&call).unwrap();
    assert!(json.get("web").is_none(), "{json}");
    let read: ToolCallEntry = serde_json::from_value(json).unwrap();
    assert_eq!(read.web, None);
    assert_eq!(read, call);
}

#[test]
fn a_tool_call_with_web_round_trips() {
    let mut call = tool_call();
    call.web = Some(Box::new(ToolCallWeb {
        search: Some(ToolCallSearch {
            query: "q".to_owned(),
            results: vec![WebResult::new("T", "https://example.com/")],
        }),
        pages: vec![ToolCallPage {
            url: "https://example.com/".to_owned(),
            title: None,
            via: PageVia::WebFetch,
        }],
    }));
    round_trips(&call);
    // A page with no title writes no `title` key.
    let json = serde_json::to_value(&call).unwrap();
    assert!(json["web"]["pages"][0].get("title").is_none(), "{json}");
}

#[test]
fn an_unknown_page_via_decodes_and_is_kept() {
    // A newer host's page kind reads as `PageVia::Unknown`, and its page is kept.
    let mut json = serde_json::to_value(tool_call()).unwrap();
    json["web"] = serde_json::json!({
        "search": null,
        "pages": [{ "url": "https://example.com/", "via": "a_future_open_page" }],
    });
    let read: ToolCallEntry = serde_json::from_value(json).unwrap();
    let page = &read.web.as_deref().unwrap().pages[0];
    assert_eq!(page.via, PageVia::Unknown);
    assert_eq!(page.url, "https://example.com/");
    assert_eq!(page.title, None);
}
