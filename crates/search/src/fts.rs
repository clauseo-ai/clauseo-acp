//! The FTS5 query text, built from what the lawyer typed. Every word is quoted, so no text
//! reaches `MATCH` as FTS5 syntax (`OR`, `NEAR(`, `col:`, `-x`, `*`, `^`, parentheses): inside a
//! quoted string FTS5 only tokenizes. Words are what whitespace separates; the tokenizer then
//! drops punctuation inside each one, so `s.` is `s` and `foo:bar` is the phrase `foo bar`.

/// At most this many words or phrases are kept; the rest of a long input is ignored.
pub(crate) const MAX_TERMS: usize = 16;

/// Words that may stand in a citation (lower case, without a final `.`).
const CITATION_WORDS: &[&str] = &[
    "s",
    "ss",
    "sec",
    "section",
    "art",
    "article",
    "order",
    "rule",
    "r",
    "o",
    "para",
    "scc",
    "air",
    "sc",
    "hc",
    "online",
    "del",
    "bom",
    "cal",
    "mad",
    "cri",
    "lj",
    "crlj",
    "धारा",
];

/// Words after which a Roman numeral in any case is a number (`order xxxix`).
const ROMAN_AFTER: &[&str] = &["order", "part", "chapter", "schedule"];

/// One term of the query: a word, or a phrase of words.
struct Term<'a> {
    words: Vec<&'a str>,
    /// Closed by a typed `"`: a finished phrase never takes a prefix `*`.
    closed: bool,
}

/// The FTS5 query for `text`, or `None` when it holds no word. With `prefix_last`, the last
/// word matches as a prefix (unless it ends a closed quote).
pub(crate) fn build(text: &str, prefix_last: bool) -> Option<String> {
    let terms = terms(text);
    let count = terms.len();
    let mut out = String::new();
    for (i, term) in terms.into_iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push('"');
        for (j, word) in term.words.iter().enumerate() {
            if j > 0 {
                out.push(' ');
            }
            out.push_str(&word.replace('"', "\"\""));
        }
        out.push('"');
        if prefix_last && i + 1 == count && !term.closed {
            out.push('*');
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Splits `text` into terms: quoted runs are phrases, citation runs become phrases, and every
/// other word is a term of its own.
fn terms(text: &str) -> Vec<Term<'_>> {
    let mut out: Vec<Term<'_>> = Vec::new();
    let mut loose: Vec<&str> = Vec::new();
    for (index, part) in text.split('"').enumerate() {
        let quoted = index % 2 == 1;
        if quoted {
            push_loose(&mut out, &mut loose);
            let words: Vec<&str> = words(part).take(MAX_TERMS).collect();
            if !words.is_empty() {
                // The part after the last `"` of an odd count is still open.
                let closed = text.split('"').count() > index + 1;
                out.push(Term { words, closed });
            }
        } else {
            loose.extend(words(part));
        }
        if out.len() >= MAX_TERMS {
            break;
        }
    }
    push_loose(&mut out, &mut loose);
    out.truncate(MAX_TERMS);
    out
}

/// Words with at least one letter or digit; a word of punctuation alone matches nothing.
/// Whitespace and control characters (Unicode Cc) break words: FTS5 stops reading a query at a
/// NUL, so one inside a quoted word would leave the quote open.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| word.chars().any(char::is_alphanumeric))
}

/// Moves the loose words into `out`, joining each citation run into one phrase.
fn push_loose<'a>(out: &mut Vec<Term<'a>>, loose: &mut Vec<&'a str>) {
    let mut i = 0;
    while i < loose.len() {
        let mut end = i;
        let mut numbers = 0;
        while end < loose.len() {
            let previous = end.checked_sub(1).and_then(|p| loose.get(p)).copied();
            match citation_part(loose[end], previous) {
                Part::Number => numbers += 1,
                Part::Word => {}
                Part::No => break,
            }
            end += 1;
        }
        if end - i >= 2 && numbers > 0 {
            out.push(Term {
                words: loose[i..end].iter().take(MAX_TERMS).copied().collect(),
                closed: false,
            });
            i = end;
        } else {
            out.push(Term {
                words: vec![loose[i]],
                closed: false,
            });
            i += 1;
        }
    }
    loose.clear();
}

enum Part {
    Number,
    Word,
    No,
}

fn citation_part(word: &str, previous: Option<&str>) -> Part {
    let plain = normal(word);
    if word.chars().any(|c| c.is_ascii_digit()) {
        return Part::Number;
    }
    let roman = !plain.is_empty()
        && plain.len() <= 8
        && plain
            .chars()
            .all(|c| matches!(c, 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'));
    let upper = word.chars().any(|c| c.is_ascii_uppercase())
        && !word.chars().any(|c| c.is_ascii_lowercase());
    if roman && (upper || previous.is_some_and(|p| ROMAN_AFTER.contains(&normal(p).as_str()))) {
        return Part::Number;
    }
    if CITATION_WORDS.contains(&plain.as_str()) {
        Part::Word
    } else {
        Part::No
    }
}

/// `word` in lower case without surrounding punctuation.
fn normal(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::build;

    #[test]
    fn words_are_quoted_and_the_last_takes_a_prefix_while_typing() {
        assert_eq!(
            build("tenant noti", true).as_deref(),
            Some("\"tenant\" \"noti\"*")
        );
        assert_eq!(
            build("tenant notice ", false).as_deref(),
            Some("\"tenant\" \"notice\"")
        );
    }

    #[test]
    fn a_citation_run_becomes_one_phrase() {
        assert_eq!(
            build("rent (2019) 5 SCC 1", false).as_deref(),
            Some("\"rent\" \"(2019) 5 SCC 1\"")
        );
        assert_eq!(
            build("s. 138 notice", false).as_deref(),
            Some("\"s. 138\" \"notice\"")
        );
        assert_eq!(
            build("order xxxix rule 1", false).as_deref(),
            Some("\"order xxxix rule 1\"")
        );
        // A keyword with no number stays a word.
        assert_eq!(
            build("order the", false).as_deref(),
            Some("\"order\" \"the\"")
        );
    }

    #[test]
    fn typed_quotes_make_a_phrase_and_a_closed_one_takes_no_prefix() {
        assert_eq!(
            build("\"SCC OnLine Del\"", true).as_deref(),
            Some("\"SCC OnLine Del\"")
        );
        assert_eq!(build("\"SCC OnLi", true).as_deref(), Some("\"SCC OnLi\"*"));
    }

    #[test]
    fn syntax_is_only_text() {
        assert_eq!(
            build("a\"b OR NEAR( foo:bar -x *", false).as_deref(),
            // The lone `*` has no letter or digit, so it is no word.
            Some("\"a\" \"b OR NEAR( foo:bar -x\"")
        );
        assert_eq!(
            build("NEAR(a b)", false).as_deref(),
            Some("\"NEAR(a\" \"b)\"")
        );
    }

    #[test]
    fn control_characters_break_words() {
        assert_eq!(build("a\0b", false).as_deref(), Some("\"a\" \"b\""));
        assert_eq!(build("tenant\0", true).as_deref(), Some("\"tenant\"*"));
        assert_eq!(build("\"a\0b\"", false).as_deref(), Some("\"a b\""));
        assert_eq!(
            build("x\u{1b}[0my", false).as_deref(),
            Some("\"x\" \"[0my\"")
        );
    }

    #[test]
    fn nothing_to_search_is_none() {
        assert_eq!(build("", true), None);
        assert_eq!(build("  - * \" ( ) ", true), None);
    }

    #[test]
    fn a_long_input_keeps_sixteen_terms() {
        let text = "word ".repeat(2000);
        let query = build(&text, false).unwrap_or_default();
        assert_eq!(query.matches("\"word\"").count(), super::MAX_TERMS);
    }
}
