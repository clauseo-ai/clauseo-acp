//! Results whose documents went to a download file (`output: "file"`, or past the server's
//! inline cap): the read's file summary as the server sends it, the listing the server is asked
//! for (`In the file:` and one line per document), and what is never a source: the file's link
//! and summaries that do not read.

use serde_json::json;

use super::tests::{golden, links, pairs, text_result};

/// The three hits the listed fixtures name, in order.
const LISTED: [(&str, &str); 3] = [
    ("Tofan Singh v. The State Of Tamil Nadu", "d143202244"),
    (
        "Justice K.S.Puttaswamy(Retd) And Anr. v. Union Of India And Ors.",
        "d91938676",
    ),
    ("Govind v. State Of Madhya Pradesh & Anr", "d436241"),
];

/// Today's server: a read saved to a file names its documents' ids, so each is a link, untitled
/// (the record shows the link as the title).
#[test]
fn a_read_saved_to_a_file_names_its_documents() {
    assert_eq!(
        golden("read_document", "ik-read-document-file.json"),
        pairs(&[("", "d143202244"), ("", "d91938676")])
    );
}

/// With the listing, the read's document keeps its title: the line comes before the summary's
/// untitled id, and a document is one link.
#[test]
fn a_listed_read_saved_to_a_file_keeps_its_title() {
    assert_eq!(
        golden("read_document", "ik-read-document-file-listed.json"),
        pairs(&LISTED[..1])
    );
}

/// A search saved to a file, with the listing: its hits, titled.
#[test]
fn a_listed_search_saved_to_a_file_names_its_hits() {
    assert_eq!(
        golden("search_law", "ik-search-law-file.json"),
        pairs(&LISTED)
    );
}

/// A JSON search past the inline cap is only a note; with the listing, its hits, titled.
#[test]
fn a_listed_json_search_past_the_cap_names_its_hits() {
    assert_eq!(
        golden("search_law", "ik-search-law-over-cap-json.json"),
        pairs(&LISTED)
    );
}

/// Today's search file summary and cap note name no document: nothing is found, the call still
/// counts, and neither the file's link in the text nor its `resource_link` is a source.
#[test]
fn a_file_result_without_a_listing_names_nothing() {
    let url = "https://data.clauseo.chat/f/2kWm8XcR5tYb9QnL3pZv6D.jsonl";
    let resource = json!({"type": "resource_link", "uri": url, "name": "search_law-page1.jsonl",
        "mimeType": "application/jsonl", "size": 4127});
    for text in [
        format!("3 hits saved as JSONL, one per line: {url} (valid 24 h).\nFields: id, title."),
        format!(
            "[Cut to fit this client's inline limit of about 8000 tokens. The full result \
             (41 KB) is at {url} ; download it instead of repeating the call.]"
        ),
    ] {
        let result =
            json!({"content": [{"type": "text", "text": text}, resource], "isError": false});
        for tool in ["search_law", "read_document"] {
            assert_eq!(links(tool, &result), [], "{tool}: {text}");
        }
    }
}

/// A summary names its documents only as the read's, on a block's first line, as a list of
/// ids alone, before a format the server writes.
#[test]
fn summaries_that_do_not_read_name_nothing() {
    let read = |text: &str| links("read_document", &text_result(text));
    assert_eq!(
        read("d1 saved as Markdown: 3 paragraphs\nhttps://data.clauseo.chat/f/x.md"),
        [super::super::found_link("https://data.clauseo.chat/d/d1", None).unwrap()]
    );
    for odd in [
        "d1, x2 saved as JSONL: 3 paragraphs",
        "d1,d2 saved as JSONL: 3 paragraphs",
        "d1 saved as PDF: 3 paragraphs",
        "d saved as JSONL: 3 paragraphs",
        "d123456789012345678901 saved as JSONL: 3 paragraphs",
        "Done.\nd1 saved as JSONL: 3 paragraphs",
        " d1 saved as JSONL: 3 paragraphs",
    ] {
        assert_eq!(read(odd), [], "{odd}");
    }
    let search = links(
        "search_law",
        &text_result("d1 saved as JSONL: 3 paragraphs"),
    );
    assert_eq!(search, []);
}
