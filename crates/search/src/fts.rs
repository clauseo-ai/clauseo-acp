//! The FTS5 query text, built from what the lawyer typed. Every word is quoted, so no text
//! reaches `MATCH` as FTS5 syntax (`OR`, `NEAR(`, `col:`, `-x`, `*`, `^`, parentheses): inside a
//! quoted string FTS5 only tokenizes. Words are what whitespace separates; the tokenizer then
//! drops punctuation inside each one, so `s.` is `s` and `foo:bar` is the phrase `foo bar`.

use crate::normal::normalise;

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
/// word matches as a prefix (unless it ends a closed quote). The text is normalised first, as
/// the stored text is. A term with digits in either script matches both spellings of them:
/// `138` and `१३८` each find the other (`("138" OR "१३८")`).
pub(crate) fn build(text: &str, prefix_last: bool) -> Option<String> {
    let text = normalise(text);
    let terms = terms(&text);
    let count = terms.len();
    let mut out = String::new();
    // A group is not an implicit-AND operand in FTS5, so next to one the terms are joined by
    // an explicit AND.
    let mut after_group = false;
    for (i, term) in terms.into_iter().enumerate() {
        let spellings = spellings(&term.words);
        let group = spellings.len() > 1;
        if i > 0 {
            out.push_str(if group || after_group { " AND " } else { " " });
        }
        after_group = group;
        let star = if prefix_last && i + 1 == count && !term.closed {
            "*"
        } else {
            ""
        };
        if let [only] = spellings.as_slice() {
            out.push_str(&quoted(only, star));
        } else {
            out.push('(');
            for (j, spelling) in spellings.iter().enumerate() {
                if j > 0 {
                    out.push_str(" OR ");
                }
                out.push_str(&quoted(spelling, star));
            }
            out.push(')');
        }
    }
    (!out.is_empty()).then_some(out)
}

/// `content` in double quotes, with `star` (`*` for a prefix) after the closing quote.
fn quoted(content: &str, star: &str) -> String {
    format!("\"{content}\"{star}")
}

/// The phrase text of `words`, as one spelling when they hold no digit; with digits, in both
/// spellings: the ASCII digits, then the Devanagari ones.
fn spellings(words: &[&str]) -> Vec<String> {
    if !words.iter().any(|word| word.chars().any(is_digit)) {
        return vec![phrase(words.iter().copied())];
    }
    vec![
        phrase(words.iter().map(|word| respell(word, true))),
        phrase(words.iter().map(|word| respell(word, false))),
    ]
}

/// The words joined by spaces, each with `"` doubled, for a quoted FTS5 string.
fn phrase<S: AsRef<str>>(words: impl Iterator<Item = S>) -> String {
    let mut out = String::new();
    for (j, word) in words.enumerate() {
        if j > 0 {
            out.push(' ');
        }
        out.push_str(&word.as_ref().replace('"', "\"\""));
    }
    out
}

/// Whether `c` is a digit in either script.
fn is_digit(c: char) -> bool {
    c.is_ascii_digit() || devanagari_digit(c).is_some()
}

/// The value of a Devanagari digit (U+0966 to U+096F).
fn devanagari_digit(c: char) -> Option<u32> {
    ('\u{0966}'..='\u{096F}')
        .contains(&c)
        .then(|| u32::from(c) - 0x0966)
}

/// `word` with its digits in one script: ASCII when `ascii`, else Devanagari.
fn respell(word: &str, ascii: bool) -> String {
    word.chars()
        .map(|c| {
            if ascii {
                devanagari_digit(c)
                    .and_then(|digit| char::from_digit(digit, 10))
                    .unwrap_or(c)
            } else {
                c.to_digit(10)
                    .and_then(|digit| char::from_u32(0x0966 + digit))
                    .unwrap_or(c)
            }
        })
        .collect()
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
    if word.chars().any(is_digit) {
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
            Some("\"rent\" AND (\"(2019) 5 SCC 1\" OR \"(२०१९) ५ SCC १\")")
        );
        assert_eq!(
            build("order xxxix rule 1", false).as_deref(),
            Some("(\"order xxxix rule 1\" OR \"order xxxix rule १\")")
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
            Some("\"x\" AND (\"[0my\" OR \"[०my\")")
        );
    }

    #[test]
    fn a_number_matches_both_digit_spellings_as_a_phrase() {
        assert_eq!(
            build("s. 138 notice", false).as_deref(),
            Some("(\"s. 138\" OR \"s. १३८\") AND \"notice\"")
        );
        assert_eq!(
            build("धारा १३८", false).as_deref(),
            Some("(\"धारा 138\" OR \"धारा १३८\")")
        );
        assert_eq!(
            build("\"धारा 138\"", false).as_deref(),
            Some("(\"धारा 138\" OR \"धारा १३८\")")
        );
        // While typed, the last number is a prefix in both spellings.
        assert_eq!(
            build("धारा 13", true).as_deref(),
            Some("(\"धारा 13\"* OR \"धारा १३\"*)")
        );
        // A word with no digit is not respelt.
        assert_eq!(build("धारा", false).as_deref(), Some("\"धारा\""));
    }

    #[test]
    fn joiners_are_removed_before_the_query_is_built() {
        assert_eq!(build("क्\u{200D}षेत्र", false).as_deref(), Some("\"क्षेत्र\""));
        // Decomposed nukta letters meet their composed spelling: the query is NFC.
        assert_eq!(
            build("\u{0928}\u{093C}ई", false).as_deref(),
            Some("\"\u{0929}ई\"")
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
