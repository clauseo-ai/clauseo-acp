//! Line numbers in Cargo manifests. `cargo metadata` gives the dependencies; these helpers find the
//! line of the manifest text that declares one, so a violation can point at it.

use crate::checks;

/// One line of a TOML file, with the most recent table header above it (`""` before the first).
pub(super) struct Line<'a> {
    pub no: u32,
    pub table: &'a str,
    pub text: &'a str,
}

/// The lines of `text`, each with its table header.
pub(super) fn lines(text: &str) -> Vec<Line<'_>> {
    let mut table = "";
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('[') {
            table = line.trim();
        }
        lines.push(Line {
            no: checks::line_at(index),
            table,
            text: line,
        });
    }
    lines
}

/// A crate's own dependency table: `[dependencies]` or `[target.<cfg>.dependencies]`.
pub(super) fn is_dependency_table(header: &str) -> bool {
    header == "[dependencies]"
        || (header.starts_with("[target.") && header.ends_with(".dependencies]"))
}

/// The root manifest's `[workspace.dependencies]` table.
pub(super) fn is_workspace_table(header: &str) -> bool {
    header == "[workspace.dependencies]"
}

/// The line that declares dependency `key`: its `key =` (or `key.`) line in a table that
/// `in_table` accepts, or a `[<table>.key]` header whose table `in_table` accepts.
pub(super) fn declaration<'a, 'l>(
    lines: &'l [Line<'a>],
    key: &str,
    in_table: impl Fn(&str) -> bool,
) -> Option<&'l Line<'a>> {
    let suffix = format!(".{key}]");
    lines.iter().find(|line| {
        let header = line
            .table
            .strip_suffix(&suffix)
            .map(|head| format!("{head}]"));
        (in_table(line.table) && declares_key(line.text, key))
            || header.is_some_and(|header| in_table(&header))
    })
}

/// The first line from `declared` onward, in the same table, that holds `needle`.
pub(super) fn in_entry<'a, 'l>(
    lines: &'l [Line<'a>],
    declared: &Line<'a>,
    needle: &str,
) -> Option<&'l Line<'a>> {
    lines
        .iter()
        .filter(|line| line.no >= declared.no && line.table == declared.table)
        .find(|line| line.text.contains(needle))
}

/// True when `line` starts an entry for `key`: `key =` or `key.`, with the key bare or double-quoted.
fn declares_key(line: &str, key: &str) -> bool {
    let line = line.trim_start();
    let rest = line.strip_prefix(key).or_else(|| {
        line.strip_prefix('"')
            .and_then(|line| line.strip_prefix(key))
            .and_then(|line| line.strip_prefix('"'))
    });
    rest.is_some_and(|rest| {
        let rest = rest.trim_start();
        rest.starts_with('=') || rest.starts_with('.')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "[dependencies]\ntauri-x = \"1\"\ntauri = { version = \"2\", features = [\n  \"devtools\",\n] }\n\n[target.'cfg(unix)'.dependencies.os]\nversion = \"2\"\n\n[workspace.dependencies]\n\"t\".workspace = true\n";

    #[test]
    fn a_key_is_found_by_its_own_line_or_its_table_header() {
        let lines = lines(TEXT);
        let tauri = declaration(&lines, "tauri", is_dependency_table).map(|line| line.no);
        assert_eq!(tauri, Some(3));
        let os = declaration(&lines, "os", is_dependency_table).map(|line| line.no);
        assert_eq!(os, Some(7));
        let quoted = declaration(&lines, "t", is_workspace_table).map(|line| line.no);
        assert_eq!(quoted, Some(11));
        assert!(declaration(&lines, "t", is_dependency_table).is_none());
    }

    #[test]
    fn a_feature_is_found_inside_its_entry() {
        let lines = lines(TEXT);
        let tauri = declaration(&lines, "tauri", is_dependency_table).expect("declared");
        assert_eq!(
            in_entry(&lines, tauri, "\"devtools\"").map(|line| line.no),
            Some(4)
        );
        assert!(in_entry(&lines, tauri, "\"protocol-asset\"").is_none());
    }
}
