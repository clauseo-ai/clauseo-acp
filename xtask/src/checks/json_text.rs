//! Line numbers in JSON text. `serde_json` parses the values; these helpers find where a key sits
//! in the text, so a violation can point at the line that sets it.

use crate::checks;
use std::ops::Range;

/// The line of the key at `path`, such as `["app", "security", "csp"]`, in the JSON `text`. Each
/// key is searched from the line of its parent key onward, so a key of the same name under another
/// object earlier in the file is not taken. Line 1 when a key on the path is not found.
pub(crate) fn key_line(text: &str, path: &[&str]) -> u32 {
    key_index(text, path).map_or(1, checks::line_at)
}

/// The 0-based index of the line `key_line` finds, or `None`.
pub(crate) fn key_index(text: &str, path: &[&str]) -> Option<usize> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = None;
    for key in path {
        let needle = format!("\"{key}\"");
        let from = found.unwrap_or(0);
        let index = lines
            .iter()
            .enumerate()
            .skip(from)
            .find(|(_, line)| declares(line, &needle))
            .map(|(index, _)| index)?;
        found = Some(index);
    }
    found
}

/// The byte ranges in `text` of the object elements, in order, of the array that the key at `path`
/// holds. The key is found as `key_line` finds it. `None` when the key is not found, its value is
/// not an array, or the array does not close. Strings are skipped with their escapes, so a brace
/// inside a string does not count.
pub(crate) fn object_elements(text: &str, path: &[&str]) -> Option<Vec<Range<usize>>> {
    let index = key_index(text, path)?;
    let line_start: usize = text.split_inclusive('\n').take(index).map(str::len).sum();
    let line = text.get(line_start..)?.lines().next()?;
    let needle = format!("\"{}\"", path.last()?);
    let after_key = line.match_indices(&needle).find_map(|(start, _)| {
        let rest = line.get(start + needle.len()..)?;
        let colon = rest.len() - rest.trim_start().len();
        rest.trim_start()
            .starts_with(':')
            .then_some(line_start + start + needle.len() + colon + 1)
    })?;
    let value = text.get(after_key..)?;
    let open = after_key + (value.len() - value.trim_start().len());
    if !text.get(open..)?.starts_with('[') {
        return None;
    }
    array_objects(text, open + 1)
}

/// The object elements of the array whose `[` sits just before byte `start`.
fn array_objects(text: &str, start: usize) -> Option<Vec<Range<usize>>> {
    let mut spans = Vec::new();
    let mut depth = 1_usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut object_start = None;
    for (offset, c) in text.get(start..)?.char_indices() {
        let at = start + offset;
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => {
                if depth == 1 && c == '{' {
                    object_start = Some(at);
                }
                depth += 1;
            }
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(spans);
                }
                if depth == 1
                    && let Some(begin) = object_start.take()
                {
                    spans.push(begin..at + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The 0-based line index of byte `offset` in `text`.
pub(crate) fn line_index_of(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .map_or(0, |head| head.matches('\n').count())
}

/// True when `line` holds `needle` (a quoted key) followed by a `:`, so a string value equal to
/// the key's name does not count.
fn declares(line: &str, needle: &str) -> bool {
    line.match_indices(needle).any(|(start, _)| {
        line.get(start + needle.len()..)
            .is_some_and(|rest| rest.trim_start().starts_with(':'))
    })
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
    fn object_elements_are_found_past_strings_with_braces() {
        let text = "{\n  \"app\": { \"caps\": [\n    \"main\",\n    { \"id\": \"a}\\\"{\", \"p\": [\"x\"] },\n    {\n      \"id\": \"b\"\n    }\n  ] }\n}";
        let spans = object_elements(text, &["app", "caps"]).expect("the array is found");
        let objects: Vec<&str> = spans
            .iter()
            .filter_map(|span| text.get(span.clone()))
            .collect();
        assert_eq!(
            objects,
            [
                "{ \"id\": \"a}\\\"{\", \"p\": [\"x\"] }",
                "{\n      \"id\": \"b\"\n    }"
            ]
        );
        assert_eq!(
            spans
                .iter()
                .map(|span| line_index_of(text, span.start))
                .collect::<Vec<_>>(),
            [3, 4]
        );
        assert!(object_elements(text, &["app", "missing"]).is_none());
        assert!(object_elements("{\"caps\": \"main\"}", &["caps"]).is_none());
    }

    #[test]
    fn a_missing_key_or_parent_falls_back_to_line_one() {
        assert_eq!(key_line(TEXT, &["app", "windows"]), 1);
        assert_eq!(key_line(TEXT, &["bundle", "csp"]), 1);
        assert_eq!(key_line(TEXT, &[]), 1);
    }
}
