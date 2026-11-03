//! Tokenizer behaviour through queries: Devanagari words stay whole, English prefixes while
//! typing, Latin accents fold, citations match as phrases, and a prefix marks the whole word.

use clauseo_search::Mode;

use crate::common::{TestDir, answer, count, find, index, marked};

const HINDI: &str = "किरायेदार ने अनुबंध की धारा 5 का उल्लंघन किया, न्यायालय में याचिका दायर।";

#[test]
fn a_devanagari_word_matches_whole_and_marks_the_whole_word() {
    let dir = TestDir::new("deva-word");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, HINDI);
    for word in ["किरायेदार", "न्यायालय", "उल्लंघन", "धारा"]
    {
        let page = find(&index, &format!("{word} "), Mode::Ranked);
        assert_eq!(page.hits.len(), 1, "{word}");
        assert_eq!(marked(&page.hits[0]), vec![word]);
    }
}

#[test]
fn a_devanagari_prefix_matches_while_typing_and_marks_the_whole_word() {
    let dir = TestDir::new("deva-prefix");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, HINDI);
    let page = find(&index, "अनु", Mode::Typing);
    assert_eq!(page.hits.len(), 1);
    assert_eq!(marked(&page.hits[0]), vec!["अनुबंध"]);
}

#[test]
fn a_devanagari_fragment_does_not_match() {
    let dir = TestDir::new("deva-fragment");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, HINDI);
    // Under the default unicode61, `किरायेदार` is stored as consonant fragments and `ना`
    // matched through the fragment `न`; a finished partial word matched the same way.
    assert_eq!(count(&index, "ना ", Mode::Ranked), 0);
    assert_eq!(count(&index, "अनुबं ", Mode::Ranked), 0);
    assert_eq!(count(&index, "र ", Mode::Ranked), 0);
}

#[test]
fn an_english_prefix_matches_while_typing_and_not_once_finished() {
    let dir = TestDir::new("english-prefix");
    let mut index = index(&dir);
    answer(
        &mut index,
        1,
        1,
        1,
        "The tenants were evicted after the notice.",
    );
    let typing = find(&index, "tenants evic", Mode::Typing);
    assert_eq!(typing.hits.len(), 1);
    assert_eq!(marked(&typing.hits[0]), vec!["tenants", "evicted"]);
    assert_eq!(count(&index, "tenants evic ", Mode::Ranked), 0);
    assert_eq!(find(&index, "Evicted ", Mode::Ranked).hits.len(), 1);
}

#[test]
fn latin_accents_fold() {
    let dir = TestDir::new("accents");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "Café résumé naïve");
    assert_eq!(find(&index, "cafe resume ", Mode::Ranked).hits.len(), 1);
    assert_eq!(find(&index, "naïve ", Mode::Ranked).hits.len(), 1);
}

#[test]
fn a_citation_matches_as_a_phrase() {
    let dir = TestDir::new("citation");
    let mut index = index(&dir);
    answer(
        &mut index,
        1,
        1,
        1,
        "See (2019) 5 SCC 1 and s. 138 of the NI Act; Order XXXIX Rule 1 CPC.",
    );
    // The same words, never next to each other.
    answer(
        &mut index,
        1,
        2,
        1,
        "In 2019 the SCC sat; volume 5 page 1. 138 pages, s for rule; order xxxix later.",
    );
    for text in [
        "(2019) 5 SCC 1 ",
        "2019 5 scc 1 ",
        "s. 138 ",
        "Order XXXIX Rule 1 ",
    ] {
        let page = find(&index, text, Mode::Ranked);
        assert_eq!(page.hits.len(), 1, "{text}");
        assert_eq!(
            page.hits[0].thread,
            Some(crate::common::thread(1)),
            "{text}"
        );
    }
}
