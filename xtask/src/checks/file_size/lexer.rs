//! A small lexer for Rust source that finds the comments. It skips string literals (escapes,
//! raw strings with any number of `#`, `b`, `c` and `br` / `cr` prefixes), char and byte
//! literals, and lifetimes and labels, so a `//` inside a string is never taken for a comment.
//! `syn` drops plain comments, so it cannot tell them apart; that is why this exists.

use crate::checks;
use std::ops::Range;

#[cfg(test)]
mod tests;

/// The last column a comment reaches on one physical line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Reach {
    /// The 1-based line.
    pub line: u32,
    /// The 1-based column, in characters, of the comment's last non-space character.
    pub column: usize,
}

/// One entry per physical line that a comment's text reaches, in source order. A line comment
/// gives one; a block comment gives one for each of its lines that holds text. Trailing spaces
/// do not count.
pub(super) fn reaches(text: &str) -> Vec<Reach> {
    let chars: Vec<char> = text.chars().collect();
    let starts = line_starts(&chars);
    comment_spans(&chars)
        .into_iter()
        .flat_map(|span| span_reaches(&chars, &starts, span))
        .collect()
}

/// The index in `chars` where each line starts; the first line starts at 0.
fn line_starts(chars: &[char]) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, &ch) in chars.iter().enumerate() {
        if ch == '\n' {
            starts.push(index + 1);
        }
    }
    starts
}

/// The 0-based line that holds the char at `index`.
fn line_of(starts: &[usize], index: usize) -> usize {
    starts
        .partition_point(|&start| start <= index)
        .saturating_sub(1)
}

/// The reaches of the comment at `span`: per physical line, the last non-space character.
fn span_reaches(chars: &[char], starts: &[usize], span: Range<usize>) -> Vec<Reach> {
    let mut found = Vec::new();
    let mut last = None;
    for index in span {
        let Some(&ch) = chars.get(index) else {
            break;
        };
        if ch == '\n' {
            found.extend(last.take());
        } else if !ch.is_whitespace() {
            let row = line_of(starts, index);
            let line_start = starts.get(row).copied().unwrap_or(0);
            last = Some(Reach {
                line: checks::line_at(row),
                column: index.saturating_sub(line_start) + 1,
            });
        }
    }
    found.extend(last);
    found
}

/// The char ranges of the comments in `chars`, in source order. A line comment runs from `//` to
/// the end of its line, the newline excluded. A block comment runs from `/*` to the `*/` that
/// closes it, nested pairs counted; one that never closes runs to the end of the text.
fn comment_spans(chars: &[char]) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut at = 0;
    while let Some(&ch) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        at = match (ch, next) {
            ('/', Some('/')) => {
                let end = line_end(chars, at);
                spans.push(at..end);
                end
            }
            ('/', Some('*')) => {
                let end = block_end(chars, at);
                spans.push(at..end);
                end
            }
            ('"', _) => string_end(chars, at + 1),
            ('\'', _) => quote_end(chars, at),
            (c, _) if is_ident_start(c) => {
                prefixed_end(chars, at).unwrap_or_else(|| ident_end(chars, at))
            }
            (c, _) if c.is_ascii_digit() => ident_end(chars, at),
            _ => at + 1,
        };
    }
    spans
}

/// The index of the newline that ends the line holding `from`, or the end of the text.
fn line_end(chars: &[char], from: usize) -> usize {
    chars
        .get(from..)
        .unwrap_or_default()
        .iter()
        .position(|&ch| ch == '\n')
        .map_or(chars.len(), |offset| from + offset)
}

/// The index just past the `*/` that closes the block comment opened at `open`, counting nested
/// `/*` pairs, or the end of the text when it never closes.
fn block_end(chars: &[char], open: usize) -> usize {
    let mut depth = 0_usize;
    let mut at = open;
    while at < chars.len() {
        if pair(chars, at, '/', '*') {
            depth += 1;
            at += 2;
        } else if pair(chars, at, '*', '/') {
            depth = depth.saturating_sub(1);
            at += 2;
            if depth == 0 {
                return at;
            }
        } else {
            at += 1;
        }
    }
    chars.len()
}

/// True when the two chars at `at` are `first` then `second`.
fn pair(chars: &[char], at: usize, first: char, second: char) -> bool {
    chars.get(at) == Some(&first) && chars.get(at + 1) == Some(&second)
}

/// The index just past the closing quote of a string whose body starts at `from`. A backslash
/// skips the char after it, so `\"` does not close the string. An unclosed string runs to the
/// end of the text.
fn string_end(chars: &[char], from: usize) -> usize {
    let mut at = from;
    while let Some(&ch) = chars.get(at) {
        match ch {
            '\\' => at += 2,
            '"' => return at + 1,
            _ => at += 1,
        }
    }
    chars.len()
}

/// The index just past the raw string whose opening quote is at `quote` and which has `hashes`
/// `#` on each side. It closes at a `"` followed by that many `#`.
fn raw_string_end(chars: &[char], quote: usize, hashes: usize) -> usize {
    let mut at = quote + 1;
    while let Some(&ch) = chars.get(at) {
        let closes = (1..=hashes).all(|offset| chars.get(at + offset) == Some(&'#'));
        if ch == '"' && closes {
            return at + 1 + hashes;
        }
        at += 1;
    }
    chars.len()
}

/// The end of a raw string whose `r` is just before `after`, when `#`s then a `"` follow it.
/// `None` for anything else, such as the raw identifier `r#type`.
fn raw_end(chars: &[char], after: usize) -> Option<usize> {
    let hashes = chars
        .get(after..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&ch| ch == '#')
        .count();
    (chars.get(after + hashes) == Some(&'"')).then(|| raw_string_end(chars, after + hashes, hashes))
}

/// The end of a literal with a prefix (`b'…'`, `b"…"`, `c"…"`, `r"…"`, `br"…"`, `cr"…"`) that
/// starts at `at`. `None` when the identifier at `at` is not such a literal.
fn prefixed_end(chars: &[char], at: usize) -> Option<usize> {
    match (chars.get(at)?, chars.get(at + 1)) {
        ('b', Some('\'')) => Some(char_literal_end(chars, at + 1)),
        ('b' | 'c', Some('"')) => Some(string_end(chars, at + 2)),
        ('b' | 'c', Some('r')) => raw_end(chars, at + 2),
        ('r', _) => raw_end(chars, at + 1),
        _ => None,
    }
}

/// The end of what starts with the quote at `quote`: a char literal, or a lifetime or label.
fn quote_end(chars: &[char], quote: usize) -> usize {
    match chars.get(quote + 1) {
        Some(&'\\') => char_literal_end(chars, quote),
        Some(_) if chars.get(quote + 2) == Some(&'\'') => quote + 3,
        Some(&ch) if is_ident_start(ch) => ident_end(chars, quote + 1),
        _ => quote + 1,
    }
}

/// The index just past the char literal whose opening quote is at `quote`, when it has an escape
/// (`'\''`, `'\\'`, `'\u{1F600}'`). An escape cannot hold a newline, so a literal that does not
/// close on its line ends there.
fn char_literal_end(chars: &[char], quote: usize) -> usize {
    if chars.get(quote + 1) != Some(&'\\') {
        return quote + 3;
    }
    let mut at = quote + 3;
    while let Some(&ch) = chars.get(at) {
        match ch {
            '\'' => return at + 1,
            '\n' => return at,
            _ => at += 1,
        }
    }
    chars.len()
}

/// The index just past the identifier, number or lifetime body that starts at `from`.
fn ident_end(chars: &[char], from: usize) -> usize {
    from + chars
        .get(from..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&ch| is_ident_continue(ch))
        .count()
}

/// True for the chars that may start an identifier.
fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}

/// True for the chars that may continue an identifier, a number or a lifetime.
fn is_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}
