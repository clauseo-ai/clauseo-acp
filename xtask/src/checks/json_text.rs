//! Line numbers in JSON text. `serde_json` parses the values; these helpers find where a key sits
//! in the text, so a violation can point at the line that sets it.

use crate::checks;

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
    fn a_missing_key_or_parent_falls_back_to_line_one() {
        assert_eq!(key_line(TEXT, &["app", "windows"]), 1);
        assert_eq!(key_line(TEXT, &["bundle", "csp"]), 1);
        assert_eq!(key_line(TEXT, &[]), 1);
    }
}
