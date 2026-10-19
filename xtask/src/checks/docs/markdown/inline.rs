//! The inline reader: code spans, inline links and the relative-link attributes of raw HTML
//! `<a>` and `<img>` tags, read left to right in one block's text.

use super::{Link, Scan, Span};

/// Reads the inline code spans and links of `source`, a block's lines joined by `\n`. A backslash
/// escapes the ASCII punctuation after it. Code spans bind tighter than brackets, so a bracket
/// inside a span is not a link. `line_of` gives the 1-based line of a byte offset.
pub(super) fn read<F: Fn(usize) -> u32>(source: &str, line_of: &F, scan: &mut Scan) {
    let bytes = source.as_bytes();
    let mut openers: Vec<usize> = Vec::new();
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'\\' && bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
            i += 2;
            continue;
        }
        match byte {
            b'`' => {
                let run = run_len(bytes, i);
                if let Some(close) = closing_run(bytes, i + run, run) {
                    scan.spans.push(Span {
                        line: line_of(i),
                        text: span_text(&source[i + run..close]),
                    });
                    i = close + run;
                } else {
                    i += run;
                }
            }
            b'[' => {
                openers.push(i);
                i += 1;
            }
            b']' => match (openers.pop(), inline_target(source, i + 1)) {
                (Some(open), Some((target, end))) => {
                    scan.links.push(Link {
                        line: line_of(open),
                        target,
                    });
                    i = end;
                }
                _ => i += 1,
            },
            b'<' => match html_tag(source, i) {
                Some((targets, end)) => {
                    scan.links.extend(targets.into_iter().map(|target| Link {
                        line: line_of(i),
                        target,
                    }));
                    i = end;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
}

/// How many backticks start at `start`.
fn run_len(bytes: &[u8], start: usize) -> usize {
    bytes
        .get(start..)
        .unwrap_or_default()
        .iter()
        .take_while(|byte| **byte == b'`')
        .count()
}

/// The index of the first backtick run of exactly `len` backticks at or after `from`.
fn closing_run(bytes: &[u8], from: usize, len: usize) -> Option<usize> {
    let mut j = from;
    while let Some(&byte) = bytes.get(j) {
        if byte == b'`' {
            let run = run_len(bytes, j);
            if run == len {
                return Some(j);
            }
            j += run;
        } else {
            j += 1;
        }
    }
    None
}

/// A code span's content as `CommonMark` gives it to a reader: line breaks as spaces, ends trimmed.
fn span_text(raw: &str) -> String {
    raw.replace('\n', " ").trim().to_owned()
}

/// The destination of the inline link whose `(` is at `at`, and the index just after its `)`.
/// `None` when the text at `at` is not an inline link: no `(`, no `)`, or a broken title.
fn inline_target(source: &str, at: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    if bytes.get(at) != Some(&b'(') {
        return None;
    }
    let start = skip_space(bytes, at + 1);
    let (target, mut i) = if bytes.get(start) == Some(&b'<') {
        let inner = source.get(start + 1..)?;
        let close = inner.find('>')?;
        let target = inner.get(..close)?;
        if target.contains('\n') {
            return None;
        }
        (target.to_owned(), start + close + 2)
    } else {
        let end = bare_end(bytes, start);
        (unescape(source.get(start..end)?), end)
    };
    i = skip_space(bytes, i);
    if matches!(bytes.get(i), Some(b'"' | b'\'' | b'(')) {
        i = title_end(bytes, i)?;
        i = skip_space(bytes, i);
    }
    (bytes.get(i) == Some(&b')')).then_some((target, i + 1))
}

/// The index of the first byte at or after `from` that is not a space, tab or line break.
fn skip_space(bytes: &[u8], from: usize) -> usize {
    let rest = bytes.get(from..).unwrap_or_default();
    from + rest
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
        .count()
}

/// The end of a bare link destination that starts at `start`: before the first space, or the
/// `)` that is not closing a `(` inside the destination.
fn bare_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'\\' && bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
            i += 2;
            continue;
        }
        match byte {
            b'(' => depth += 1,
            b')' if depth == 0 => break,
            b')' => depth -= 1,
            b' ' | b'\t' | b'\n' => break,
            _ => {}
        }
        i += 1;
    }
    i
}

/// The index just after the title that opens at `open` (a quote or a `(`), `None` if it never
/// closes.
fn title_end(bytes: &[u8], open: usize) -> Option<usize> {
    let close = if bytes.get(open) == Some(&b'(') {
        b')'
    } else {
        *bytes.get(open)?
    };
    let mut i = open + 1;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'\\' && bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
            i += 2;
            continue;
        }
        if byte == close {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// `text` with each backslash before ASCII punctuation removed, as a destination reads it.
pub(super) fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(next)) if next.is_ascii_punctuation() => {
                out.push(*next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// The `href` of an `<a>` tag or the `src` of an `<img>` tag that starts at `at`, as the values
/// it holds (empty ones dropped), with the index just after the tag's `>`. `None` when the text
/// at `at` is not such a tag.
fn html_tag(source: &str, at: usize) -> Option<(Vec<String>, usize)> {
    let bytes = source.as_bytes();
    let name_start = at + 1;
    let name_len = bytes
        .get(name_start..)?
        .iter()
        .take_while(|byte| byte.is_ascii_alphanumeric())
        .count();
    let name = source
        .get(name_start..name_start + name_len)?
        .to_ascii_lowercase();
    let wanted = match name.as_str() {
        "a" => "href",
        "img" => "src",
        _ => return None,
    };
    let after_name = name_start + name_len;
    if !matches!(
        bytes.get(after_name),
        Some(b' ' | b'\t' | b'\n' | b'>' | b'/')
    ) {
        return None;
    }
    let mut targets = Vec::new();
    let mut i = after_name;
    loop {
        i = skip_space(bytes, i);
        match bytes.get(i)? {
            b'>' => return Some((targets, i + 1)),
            b'/' => i += 1,
            _ => {
                let (attribute, value, next) = attribute(source, i)?;
                if attribute.eq_ignore_ascii_case(wanted) && !value.is_empty() {
                    targets.push(value.to_owned());
                }
                i = next;
            }
        }
    }
}

/// The attribute that starts at `start`: its name, its value (empty when it has none) and the
/// index after it. A stray `=` is read as an empty name, so the caller always moves on.
fn attribute(source: &str, start: usize) -> Option<(&str, &str, usize)> {
    let bytes = source.as_bytes();
    let name_len = bytes
        .get(start..)?
        .iter()
        .take_while(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'=' | b'>' | b'/'))
        .count();
    let name = source.get(start..start + name_len)?;
    if name.is_empty() {
        return Some(("", "", start + 1));
    }
    let end_of_name = start + name_len;
    let equals = skip_space(bytes, end_of_name);
    if bytes.get(equals) != Some(&b'=') {
        return Some((name, "", end_of_name));
    }
    let value_start = skip_space(bytes, equals + 1);
    if let Some(&quote @ (b'"' | b'\'')) = bytes.get(value_start) {
        let inner = source.get(value_start + 1..)?;
        let close = inner.find(char::from(quote))?;
        return Some((name, inner.get(..close)?, value_start + close + 2));
    }
    let len = bytes
        .get(value_start..)?
        .iter()
        .take_while(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | b'>'))
        .count();
    let value = source.get(value_start..value_start + len)?;
    Some((name, value, value_start + len))
}
