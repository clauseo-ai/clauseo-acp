use super::{MAX_NAME_CHARS, clean_title};

fn cleaned(title: &str) -> Option<String> {
    clean_title(title).map(|name| name.to_string())
}

#[test]
fn a_plain_title_is_kept() {
    assert_eq!(cleaned("Gemini CLI").as_deref(), Some("Gemini CLI"));
    assert_eq!(
        cleaned("Claude Code 2.1").as_deref(),
        Some("Claude Code 2.1")
    );
}

#[test]
fn surrounding_and_inner_whitespace_collapse_to_one_space() {
    assert_eq!(cleaned("  Gemini   CLI \n").as_deref(), Some("Gemini CLI"));
    assert_eq!(cleaned("Gemini\tCLI").as_deref(), Some("Gemini CLI"));
    assert_eq!(cleaned("a \t\r\n b").as_deref(), Some("a b"));
    assert_eq!(cleaned("a\u{2028}b").as_deref(), Some("a b"));
}

#[test]
fn control_characters_are_dropped() {
    assert_eq!(cleaned("Gem\u{0}ini\u{7}").as_deref(), Some("Gemini"));
    assert_eq!(
        cleaned("\u{1B}[31mRed\u{1B}[0m").as_deref(),
        Some("[31mRed[0m")
    );
    assert_eq!(cleaned("a\u{7F}b").as_deref(), Some("ab"));
}

#[test]
fn bidi_embedding_override_and_isolate_characters_are_dropped() {
    for bidi in ['\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}', '\u{202E}'] {
        let title = format!("Gem{bidi}ini");
        assert_eq!(cleaned(&title).as_deref(), Some("Gemini"), "{bidi:?}");
    }
    for bidi in ['\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}'] {
        let title = format!("Gem{bidi}ini");
        assert_eq!(cleaned(&title).as_deref(), Some("Gemini"), "{bidi:?}");
    }
    // The bidi marks outside those ranges are not in the rule, so they stay.
    assert_eq!(cleaned("a\u{200E}b").as_deref(), Some("a\u{200E}b"));
}

#[test]
fn a_title_is_cut_to_64_chars() {
    let long: String = "é".repeat(MAX_NAME_CHARS + 10);
    let shown = cleaned(&long).unwrap();
    assert_eq!(shown.chars().count(), MAX_NAME_CHARS);
    let exact: String = "x".repeat(MAX_NAME_CHARS);
    assert_eq!(cleaned(&exact), Some(exact.clone()));
}

#[test]
fn a_cut_that_ends_on_a_space_drops_it() {
    let title = format!("{} tail", "x".repeat(MAX_NAME_CHARS - 1));
    assert_eq!(cleaned(&title), Some("x".repeat(MAX_NAME_CHARS - 1)));
}

#[test]
fn empty_after_cleaning_is_none() {
    assert_eq!(cleaned(""), None);
    assert_eq!(cleaned("   \t\n "), None);
    assert_eq!(cleaned("\u{0}\u{202E}\u{2069}"), None);
    assert_eq!(cleaned("\u{7}  \u{7}"), None);
}
