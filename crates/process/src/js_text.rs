//! JavaScript's whitespace, as `String.prototype.trim` strips it: the set Claude Code (a
//! JavaScript program) trims a value by before it reads it, so Rust that decides what Claude
//! Code will make of a value trims by the same set. Rust's own `str::trim` differs in two
//! characters: it strips U+0085 (NEXT LINE), which JavaScript keeps, and keeps U+FEFF (the byte
//! order mark), which JavaScript strips.

/// Whether `c` is JavaScript whitespace (ECMAScript's `WhiteSpace` and `LineTerminator`): tab,
/// line feed, vertical tab, form feed, carriage return, space, U+00A0, U+1680, U+2000 to U+200A,
/// U+2028, U+2029, U+202F, U+205F, U+3000 and U+FEFF. Never U+0085.
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `text` with JavaScript whitespace ([`is_js_whitespace`]) stripped from both ends, as
/// JavaScript's `text.trim()` gives it.
pub fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The set, exactly: every `char` is checked against the list.
    #[test]
    fn js_whitespace_is_exactly_javascript_s_set() {
        let mut listed: Vec<char> = "\t\n\u{0B}\u{0C}\r \u{A0}\u{1680}".chars().collect();
        listed.extend('\u{2000}'..='\u{200A}');
        listed.extend([
            '\u{2028}', '\u{2029}', '\u{202F}', '\u{205F}', '\u{3000}', '\u{FEFF}',
        ]);
        let found: Vec<char> = (char::MIN..=char::MAX)
            .filter(|c| is_js_whitespace(*c))
            .collect();
        assert_eq!(found, listed);
        assert_eq!(found.len(), 25);
        assert!(!is_js_whitespace('\u{85}'));
        assert!(!is_js_whitespace('\u{180E}'));
        assert!(!is_js_whitespace('\u{200B}'));
    }

    #[test]
    fn js_trim_strips_the_byte_order_mark_and_keeps_next_line() {
        assert_eq!(js_trim("\u{FEFF}1\u{3000}"), "1");
        assert_eq!(js_trim(" \t\u{0B}on\u{2028}\u{A0}"), "on");
        assert_eq!(js_trim("\u{85}1"), "\u{85}1");
        assert_eq!(js_trim("1\u{85}"), "1\u{85}");
        assert_eq!(js_trim(" a b "), "a b");
        assert_eq!(js_trim("\u{FEFF}\u{3000} "), "");
        assert_eq!(js_trim(""), "");
    }
}
