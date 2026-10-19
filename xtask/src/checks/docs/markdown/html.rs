//! The relative-link attributes of raw HTML: the `href` of an `<a>` tag and the `src` of an
//! `<img>` tag, read from the text of `Html` and `InlineHtml` events.

use super::{Link, Scan};

/// Adds a link for each `<a href>` and `<img src>` in `html`. `line_of` gives the 1-based line
/// of a byte offset in `html`.
pub(super) fn read_tags(html: &str, line_of: &dyn Fn(usize) -> u32, scan: &mut Scan) {
    let mut i = 0;
    while let Some(open) = html.get(i..).and_then(|rest| rest.find('<')) {
        let at = i + open;
        match html_tag(html, at) {
            Some((targets, end)) => {
                scan.links.extend(targets.into_iter().map(|target| Link {
                    line: line_of(at),
                    target,
                }));
                i = end;
            }
            None => i = at + 1,
        }
    }
}

/// The index of the first byte at or after `from` that is not a space, tab or line break.
fn skip_space(bytes: &[u8], from: usize) -> usize {
    let rest = bytes.get(from..).unwrap_or_default();
    from + rest
        .iter()
        .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\n'))
        .count()
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
