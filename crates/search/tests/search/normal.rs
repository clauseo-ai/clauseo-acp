//! One normalisation for the stored text and the query: Unicode NFC (nukta letters in either
//! encoding meet), the zero-width joiners removed, Devanagari digits and ASCII digits matching
//! each other, and the tokenizer's Devanagari marks left alone.

use clauseo_search::{Mode, Page};

use crate::common::{TestDir, answer, at, find, index, marked, matter, snippet, thread};

/// NFD of `text`: every precomposed character split into its base and its marks.
fn nfd(text: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    text.nfd().collect()
}

/// The hits' entries (thread numbers the helper took), in page order.
fn threads(page: &Page) -> Vec<clauseo_protocol::ThreadId> {
    page.hits.iter().filter_map(|hit| hit.thread).collect()
}

#[test]
fn a_phrase_with_digits_finds_either_digit_spelling() {
    let dir = TestDir::new("norm-digits");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "धारा 138 के तहत नोटिस दिया गया।");
    answer(&mut index, 1, 2, 1, "धारा १३८ के तहत याचिका।");
    answer(&mut index, 1, 3, 1, "धारा 139 के तहत अन्य।");
    for text in ["धारा 138 ", "धारा १३८ ", "\"धारा 138\" ", "\"धारा १३८\" "]
    {
        let page = find(&index, text, Mode::Ranked);
        assert_eq!(page.hits.len(), 2, "{text}");
        assert!(threads(&page).contains(&thread(1)), "{text}");
        assert!(threads(&page).contains(&thread(2)), "{text}");
    }
    // A single number matches its other spelling too.
    assert_eq!(find(&index, "138 ", Mode::Ranked).hits.len(), 2);
    assert_eq!(find(&index, "१३८ ", Mode::Ranked).hits.len(), 2);
    // While a number is typed, its prefix reaches both spellings (and 139, a prefix too).
    assert_eq!(find(&index, "धारा 13", Mode::Typing).hits.len(), 3);
    assert_eq!(find(&index, "धारा १३", Mode::Typing).hits.len(), 3);
}

#[test]
fn a_nukta_letter_matches_in_either_encoding() {
    let dir = TestDir::new("norm-nukta");
    let mut index = index(&dir);
    // The nukta letter क़ (U+0915 U+093C) stored decomposed, as keyboards and macOS file names
    // give it.
    answer(&mut index, 1, 1, 1, &nfd("क़ानून की बात"));
    // The same letter stored precomposed as U+0958, the nukta letters' own code point.
    answer(&mut index, 1, 2, 1, "\u{0958}ानून की बात");
    // U+0929 (ऩ) stored decomposed as U+0928 U+093C; U+0929 is the composed form.
    answer(&mut index, 1, 3, 1, &nfd("\u{0929}ई"));
    assert_eq!(find(&index, "\u{0958}ानून ", Mode::Ranked).hits.len(), 2);
    assert_eq!(find(&index, &nfd("क़ानून "), Mode::Ranked).hits.len(), 2);
    assert_eq!(find(&index, "\u{0929}ई ", Mode::Ranked).hits.len(), 1);
    assert_eq!(find(&index, &nfd("\u{0929}ई "), Mode::Ranked).hits.len(), 1);
}

#[test]
fn a_word_with_a_joiner_matches_the_word_without_it() {
    let dir = TestDir::new("norm-joiner");
    let mut index = index(&dir);
    // Half forms written with an explicit ZWJ (U+200D) and a ZWNJ (U+200C).
    answer(&mut index, 1, 1, 1, "सुप्रीम कोर्ट में क्\u{200D}षेत्र का फ़ैसला");
    answer(&mut index, 1, 2, 1, "नियम क्\u{200C}त्र का");
    answer(&mut index, 1, 3, 1, "क्षेत्र और नियम क्त्र");
    // Typed without the joiners, the word is found in all three.
    assert_eq!(find(&index, "क्षेत्र ", Mode::Ranked).hits.len(), 2);
    assert_eq!(find(&index, "क्त्र ", Mode::Ranked).hits.len(), 2);
    // Typed with a ZWJ, it is found in the same texts.
    assert_eq!(find(&index, "क्\u{200D}षेत्र ", Mode::Ranked).hits.len(), 2);
}

#[test]
fn the_snippet_shows_the_normalised_text() {
    let dir = TestDir::new("norm-snippet");
    let mut index = index(&dir);
    answer(&mut index, 1, 1, 1, "नियम क्\u{200D}षेत्र और \u{0958}ानून");
    let page = find(&index, "क्षेत्र ", Mode::Ranked);
    assert_eq!(page.hits.len(), 1);
    assert_eq!(snippet(&page.hits[0]), "नियम क्षेत्र और क़ानून");
    assert_eq!(marked(&page.hits[0]), vec!["क्षेत्र"]);
}

#[test]
fn a_joiner_between_a_base_and_its_mark_does_not_block_the_composition() {
    let dir = TestDir::new("norm-joiner-compose");
    let mut index = index(&dir);
    // e, a ZWJ, then the combining acute: once the joiner is gone the pair composes to é.
    answer(&mut index, 1, 1, 1, "le e\u{200D}\u{301}crit ici");
    let page = find(&index, "ecrit ", Mode::Ranked);
    assert_eq!(page.hits.len(), 1);
    assert_eq!(snippet(&page.hits[0]), "le \u{e9}crit ici");
}

#[test]
fn a_devanagari_file_name_in_nfd_is_found_by_an_nfc_query() {
    let dir = TestDir::new("norm-files");
    let mut index = index(&dir);
    let path = nfd("अनुबंध/किरायानामा (ऩ).docx");
    index
        .put_file_names(matter(1), at(1), [path.as_str()])
        .unwrap();
    assert_eq!(find(&index, "अनुबंध ", Mode::Ranked).hits.len(), 1);
    assert_eq!(find(&index, "\u{0929} ", Mode::Ranked).hits.len(), 1);
    assert_eq!(find(&index, "किरायानामा ", Mode::Ranked).hits.len(), 1);
    // The hit carries the path as it was put: the host opens files by the bytes it gave.
    let page = find(&index, "अनुबंध ", Mode::Ranked);
    assert_eq!(page.hits[0].path.as_deref(), Some(path.as_str()));
}

#[test]
fn a_thread_and_a_matter_title_are_normalised_too() {
    let dir = TestDir::new("norm-titles");
    let mut index = index(&dir);
    index
        .put_matter_title(matter(1), at(1), &nfd("क़िराया मामला"))
        .unwrap();
    index
        .put_thread_title(matter(1), thread(1), at(2), "नियम क्\u{200D}षेत्र")
        .unwrap();
    assert_eq!(find(&index, "\u{0958}िराया ", Mode::Ranked).hits.len(), 1);
    assert_eq!(find(&index, "क्षेत्र ", Mode::Ranked).hits.len(), 1);
}

/// The tokenizer `remove_diacritics 2` with `categories 'L* N* Co M*'` keeps Devanagari
/// vowel signs, the anusvara, the virama and the nukta: a word that differs from another only
/// in one of them does not match it, so none of them is folded away.
#[test]
fn remove_diacritics_keeps_the_devanagari_marks() {
    let dir = TestDir::new("norm-marks");
    let mut index = index(&dir);
    // Each stored word, and a word that differs from it only in one mark (or lacks it).
    let pairs = [
        ("कि", "की"),
        ("कु", "कू"),
        ("कं", "क"),
        ("क्", "क"),
        ("\u{0915}\u{093C}", "क"),
        ("का", "क"),
    ];
    for (n, (stored, _)) in (1_u64..).zip(pairs.iter()) {
        answer(&mut index, 1, n, 1, &format!("{stored} शब्द"));
    }
    for (stored, other) in pairs {
        assert!(
            find(&index, &format!("{other} "), Mode::Ranked)
                .hits
                .is_empty(),
            "{other} must not match {stored}"
        );
        let page = find(&index, &format!("{stored} "), Mode::Ranked);
        assert_eq!(page.hits.len(), 1, "{stored}");
        assert_eq!(
            marked(&page.hits[0]),
            vec![stored],
            "{stored} is marked whole"
        );
    }
}
