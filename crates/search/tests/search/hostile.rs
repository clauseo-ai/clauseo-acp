//! Text that would be FTS5 syntax is only text: it never errors and never acts as an operator.

use clauseo_search::Mode;

use crate::common::{TestDir, answer, count, find, index, marked, snippet};

#[test]
fn hostile_text_never_errors() {
    let dir = TestDir::new("hostile");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "plain words about a tenant and rent");
    let long = format!("{}\"", "x\"(NEAR *".repeat(1_100));
    let inputs = [
        "\"",
        "\"\"",
        "a\"b",
        "\"unbalanced",
        "OR",
        "AND NOT",
        "NEAR(",
        "NEAR(tenant rent, 2)",
        "*",
        "tenant*",
        "-tenant",
        "body:tenant",
        "{body}: tenant",
        "(tenant OR rent)",
        "^tenant",
        "tenant +rent",
        "\u{2}tenant\u{3}",
        "'; DROP TABLE items; --",
        "a\0b",
        "tenant\0",
        "\"a\0b\"",
        "\0",
        "ten\u{1b}ant\u{7f}",
        long.as_str(),
    ];
    for text in inputs {
        for mode in [Mode::Typing, Mode::Ranked] {
            let result = index.query(&crate::common::query(text, mode));
            assert!(result.is_ok(), "{text:.40} in {mode:?}: {result:?}");
        }
    }
}

#[test]
fn a_nul_breaks_words_instead_of_ending_the_query() {
    let dir = TestDir::new("nul");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "the tenant paid");
    for mode in [Mode::Typing, Mode::Ranked] {
        // FTS5 stops reading at a NUL: inside a quoted word it left the quote open, an error.
        let page = index
            .query(&crate::common::query("tenant\0paid ", mode))
            .unwrap();
        assert_eq!(page.hits.len(), 1, "{mode:?}");
        assert_eq!(marked(&page.hits[0]), vec!["tenant", "paid"], "{mode:?}");
    }
}

#[test]
fn operators_are_matched_as_words_not_obeyed() {
    let dir = TestDir::new("operators");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "the tenant paid");
    answer(&mut index, 1, 2, 1, "the landlord asked");
    // As FTS5 syntax each would match thread 1 or both; as words, nothing has them all.
    for text in [
        "tenant OR landlord ",
        "NEAR(tenant paid) ",
        "landlord NOT asked ",
        "body:tenant ",
    ] {
        let page = find(&index, text, Mode::Ranked);
        assert!(page.hits.is_empty(), "{text}: {:?}", page.hits);
    }
    // `-tenant` is the word tenant, not `NOT tenant`.
    let page = find(&index, "-tenant ", Mode::Ranked);
    assert_eq!(page.hits.len(), 1);
    // A typed `*` is punctuation: `ten*` finished is the word `ten`, which nothing holds.
    assert_eq!(count(&index, "ten* ", Mode::Ranked), 0);
}

#[test]
fn snippet_parts_rejoin_to_the_text_with_the_query_words_marked() {
    let dir = TestDir::new("snippet");
    let mut index = index(&dir);
    let text = "The tenant paid the rent late, and the landlord served a notice on the tenant.";
    answer(&mut index, 1, 1, 1, text);
    for mode in [Mode::Ranked, Mode::Typing] {
        let page = find(&index, "tenant notice ", mode);
        let hit = &page.hits[0];
        assert_eq!(snippet(hit), text, "{mode:?}");
        assert_eq!(marked(hit), vec!["tenant", "notice", "tenant"], "{mode:?}");
    }
}

#[test]
fn a_long_entry_gives_a_bounded_snippet_around_the_match() {
    let dir = TestDir::new("snippet-long");
    let mut index = index(&dir);
    let text = format!(
        "{} arbitration {}",
        "filler ".repeat(400),
        "tail ".repeat(400)
    );
    answer(&mut index, 1, 1, 1, &text);
    let page = find(&index, "arbitration ", Mode::Ranked);
    let hit = &page.hits[0];
    let shown = snippet(hit);
    assert!(shown.chars().count() <= clauseo_search::MAX_SNIPPET_CHARS);
    assert!(shown.contains("arbitration"));
    assert_eq!(marked(hit), vec!["arbitration"]);
}
