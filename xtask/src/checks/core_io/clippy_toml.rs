//! What a held crate's `clippy.toml` must parse to: the root `clippy.toml` with the arrays of
//! `xtask/core-io-bans.toml` appended to its own, and the text of that file for the fix line.

use super::report;
use crate::checks;
use crate::violation::Violation;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use toml::{Spanned, Table, Value};

/// The list of banned items, repo-relative.
pub(crate) const LIST: &str = "xtask/core-io-bans.toml";

/// The root clippy config, repo-relative.
pub(crate) const ROOT: &str = "clippy.toml";

/// The arrays the list holds, in the order the generated file writes them.
const KEYS: [&str; 2] = ["disallowed-methods", "disallowed-types"];

/// The list as clippy reads its entries; any other key fails to parse.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    #[serde(rename = "disallowed-methods", default)]
    methods: Vec<Spanned<Entry>>,
    #[serde(rename = "disallowed-types", default)]
    types: Vec<Spanned<Entry>>,
}

/// One entry of `disallowed-methods` or `disallowed-types`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    reason: String,
    #[expect(dead_code, reason = "read only so that the key parses; clippy uses it")]
    replacement: Option<String>,
    #[serde(rename = "allow-invalid")]
    #[expect(dead_code, reason = "read only so that the key parses; clippy uses it")]
    allow_invalid: Option<bool>,
}

/// What every held crate's `clippy.toml` must be.
#[derive(Debug)]
pub(super) struct Expected {
    /// The parsed TOML the crate's file must equal.
    pub table: Table,
    /// The text of the file to write: `table`, with a header saying where it comes from.
    pub text: String,
}

/// Reads the root `clippy.toml` and the list under `root` and merges them. `Err` holds the
/// problems with either file, which stop the check.
pub(super) fn load(root: &Path) -> Result<Expected, Vec<Violation>> {
    let root_text = read(root, ROOT)?;
    let list_text = read(root, LIST)?;
    let mut problems = check_list(&list_text);
    let root_table = parse_spanned(ROOT, &root_text);
    let list_table = parse_spanned(LIST, &list_text);
    let (mut table, list_table) = match (root_table, list_table) {
        (Ok(root_table), Ok(list_table)) => (root_table, list_table),
        (root_table, list_table) => {
            problems.extend(root_table.err());
            problems.extend(list_table.err());
            return Err(problems);
        }
    };
    for key in KEYS {
        let Some(Value::Array(added)) = list_table.get(key).map(Spanned::get_ref) else {
            continue;
        };
        let Some(own) = table.get_mut(key) else {
            table.insert(
                key.to_owned(),
                Spanned::new(0..0, Value::Array(added.clone())),
            );
            continue;
        };
        let line = line_number(&root_text, own.span().start);
        match own.get_mut() {
            Value::Array(own) => own.extend(added.iter().cloned()),
            _ => problems.push(report::root_not_an_array(key, line)),
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let table: Table = table
        .into_iter()
        .map(|(key, value)| (key, value.into_inner()))
        .collect();
    let text = render(&table);
    Ok(Expected { table, text })
}

/// The text of `rel` under `root`; a missing file is a problem at its line 1.
fn read(root: &Path, rel: &str) -> Result<String, Vec<Violation>> {
    match checks::read_repo_file(root, rel) {
        Ok(Some(text)) => Ok(text),
        Ok(None) => Err(vec![report::missing_input(rel)]),
        Err(unreadable) => Err(vec![unreadable]),
    }
}

/// `text` of the file `rel` as top-level keys with the span of each value.
fn parse_spanned(rel: &str, text: &str) -> Result<BTreeMap<String, Spanned<Value>>, Violation> {
    toml::from_str(text).map_err(|error| not_toml(rel, text, &error))
}

/// The problems of the list's own shape: a key clippy would not read, an entry without a path
/// or a reason, an empty reason, a path listed twice.
fn check_list(text: &str) -> Vec<Violation> {
    let list: List = match toml::from_str(text) {
        Ok(list) => list,
        // Text that is not TOML at all is `parse_spanned`'s to report.
        Err(_) if toml::from_str::<Table>(text).is_err() => return Vec::new(),
        Err(error) => return vec![not_toml(LIST, text, &error)],
    };
    let mut problems = Vec::new();
    for (key, entries) in [(KEYS[0], &list.methods), (KEYS[1], &list.types)] {
        let mut seen = BTreeSet::new();
        for entry in entries {
            let line = line_number(text, entry.span().start);
            let Entry { path, reason, .. } = entry.get_ref();
            if reason.trim().is_empty() {
                problems.push(report::empty_reason(key, path, line));
            }
            if !seen.insert(path.as_str()) {
                problems.push(report::listed_twice(key, path, line));
            }
        }
    }
    problems
}

/// The problem for `text` of the file `rel`, which `toml` could not read.
fn not_toml(rel: &str, text: &str, error: &toml::de::Error) -> Violation {
    let line = error.span().map_or(1, |span| line_number(text, span.start));
    report::not_toml(rel, line, error)
}

/// The 1-based line that byte `offset` of `text` is on.
pub(super) fn line_number(text: &str, offset: usize) -> u32 {
    let head = text.get(..offset).unwrap_or(text);
    u32::try_from(head.matches('\n').count() + 1).unwrap_or(u32::MAX)
}

/// The text of a held crate's `clippy.toml`: a header, every key that is not an array of tables
/// as `key = value`, then each array of tables with one inline table per line.
pub(super) fn render(table: &Table) -> String {
    let mut lines = vec![format!(
        "# Generated: the root {ROOT} with the arrays of {LIST} appended, root\n\
         # entries first, since clippy reads only the nearest clippy.toml. `cargo xtask check`\n\
         # (core-io) fails when this file parses to anything else and prints the file to write."
    )];
    let is_rows =
        |value: &Value| matches!(value, Value::Array(rows) if rows.iter().all(Value::is_table));
    for (key, value) in table.iter().filter(|(_, value)| !is_rows(value)) {
        lines.push(format!("{} = {value}", toml_key(key)));
    }
    for (key, value) in table.iter().filter(|(_, value)| is_rows(value)) {
        lines.push(format!("{} = [", toml_key(key)));
        let rows = value.as_array().into_iter().flatten();
        for row in rows.filter_map(Value::as_table) {
            let cells: Vec<String> = row
                .iter()
                .map(|(name, cell)| format!("{} = {cell}", toml_key(name)))
                .collect();
            lines.push(format!("  {{ {} }},", cells.join(", ")));
        }
        lines.push("]".to_owned());
    }
    lines.push(String::new());
    lines.join("\n")
}

/// `key` as TOML writes it: bare when it can be, else quoted.
fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if bare {
        key.to_owned()
    } else {
        Value::String(key.to_owned()).to_string()
    }
}

/// The top-level keys whose values differ between `found` and `expected`, in key order.
pub(super) fn differing_keys(found: &Table, expected: &Table) -> Vec<String> {
    let keys: BTreeSet<&String> = found.keys().chain(expected.keys()).collect();
    keys.into_iter()
        .filter(|key| found.get(*key) != expected.get(*key))
        .cloned()
        .collect()
}
