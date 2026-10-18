//! Line numbers in JSON text. `serde_json` parses the values; these helpers find where a key sits
//! in the text, so a violation can point at the line that sets it.

use crate::checks;

/// The line of the key at `path`, such as `["app", "security", "csp"]`, in the JSON `text`. Each
/// key is searched only among the keys of its parent's object, up to the brace that closes it, so
/// a key of the same name under another object is not taken. Line 1 when a key on the path is not
/// found.
pub(crate) fn key_line(text: &str, path: &[&str]) -> u32 {
    key_index(text, path).map_or(1, checks::line_at)
}

/// The 0-based index of the line `key_line` finds, or `None`.
pub(crate) fn key_index(text: &str, path: &[&str]) -> Option<usize> {
    let mut object = 0;
    let mut key_at = None;
    for key in path {
        let (start, value) = member(text, object, key)?;
        key_at = Some(start);
        object = value;
    }
    key_at.map(|offset| line_index_of(text, offset))
}

/// The key `key` among the direct members of the object that opens at or after byte `from` of
/// `text`: the byte offset of its opening quote and of its value. The search ends at the brace that
/// closes the object. Strings are skipped with their escapes, so a brace inside one does not
/// count. A key is compared as written, escapes and all.
fn member(text: &str, from: usize, key: &str) -> Option<(usize, usize)> {
    let rest = text.get(from..)?;
    let open = from + rest.find(|c: char| !c.is_whitespace())?;
    if !text.get(open..)?.starts_with('{') {
        return None;
    }
    let mut depth = 0_usize;
    let mut chars = text.get(open..)?.char_indices();
    while let Some((offset, c)) = chars.next() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return None;
                }
            }
            '"' => {
                let start = open + offset;
                let end = string_end(text, start)?;
                while chars.next().is_some_and(|(next, _)| open + next + 1 < end) {}
                let after = text.get(end..)?;
                let colon = after.len() - after.trim_start().len();
                if depth == 1
                    && after.trim_start().starts_with(':')
                    && text.get(start + 1..end - 1) == Some(key)
                {
                    return Some((start, end + colon + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// The byte just past the closing quote of the string whose opening quote is at `start`.
fn string_end(text: &str, start: usize) -> Option<usize> {
    let mut escaped = false;
    for (offset, c) in text.get(start + 1..)?.char_indices() {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(start + 1 + offset + 1);
        }
    }
    None
}

/// The 0-based line index of byte `offset` in `text`.
pub(crate) fn line_index_of(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .map_or(0, |head| head.matches('\n').count())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = r#"{
  "build": {
    "withGlobalTauri": true
  },
  "app": {
    "label": "withGlobalTauri",
    "withGlobalTauri": false,
    "security": { "csp": "x" }
  }
}"#;

    #[test]
    fn a_key_is_found_under_its_parent_not_under_another_object() {
        assert_eq!(key_line(TEXT, &["app", "withGlobalTauri"]), 7);
        assert_eq!(key_line(TEXT, &["app", "security", "csp"]), 8);
    }

    #[test]
    fn the_search_stops_at_the_end_of_the_parent_object() {
        let text = "{\n  \"app\": {\n    \"security\": {\n      \"freezePrototype\": true\n    }\n  },\n  \"plugins\": {\n    \"x\": { \"csp\": \"y\" }\n  }\n}";
        assert_eq!(key_line(text, &["app", "security", "csp"]), 1);
        assert_eq!(key_line(text, &["plugins", "x", "csp"]), 8);
    }

    #[test]
    fn a_key_inside_a_string_or_a_nested_object_is_not_a_member() {
        let text = "{\n  \"a\": \"\\\"b\\\": {\",\n  \"c\": { \"b\": 1 },\n  \"b\": 2\n}";
        assert_eq!(key_line(text, &["b"]), 4);
    }

    #[test]
    fn a_missing_key_or_parent_falls_back_to_line_one() {
        assert_eq!(key_line(TEXT, &["app", "windows"]), 1);
        assert_eq!(key_line(TEXT, &["bundle", "csp"]), 1);
        assert_eq!(key_line(TEXT, &[]), 1);
    }
}
