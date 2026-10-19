//! What else decides whether clippy reports the calls the list bans: the lint levels. A held
//! crate's `Cargo.toml` must inherit the workspace lints (`[lints] workspace = true`, beside which
//! cargo takes no other lint key), the root `[workspace.lints.clippy]` may set
//! `disallowed_methods` and `disallowed_types` only to a level that reports, and the repo's
//! `.cargo/config.toml` may not name them in rustflags. `#![allow(..)]` in the code is denied by
//! the workspace lint `allow_attributes`; `#[expect(..)]` is the exception route.

use super::report;
use crate::checks;
use crate::violation::Violation;
use crate::workspace::Member;
use std::path::Path;
use toml::Spanned;
use toml::de::{DeString, DeTable, DeValue};

/// The lints the list feeds, as cargo and rustc write them with `_`.
const LINTS: [&str; 2] = ["disallowed_methods", "disallowed_types"];

/// The levels at which those lints report a call.
const REPORTING: [&str; 3] = ["warn", "deny", "forbid"];

/// The repo's root manifest.
pub(super) const ROOT_MANIFEST: &str = "Cargo.toml";

/// The repo's cargo config, the one file cargo reads its rustflags from inside the repo.
pub(super) const CARGO_CONFIG: &str = ".cargo/config.toml";

/// The violation for the held crate `member` when its `Cargo.toml` does not say
/// `[lints] workspace = true`.
pub(super) fn crate_manifest(root: &Path, member: &Member) -> Vec<Violation> {
    let path = checks::join(&member.dir, "Cargo.toml");
    let text = match checks::read_repo_file(root, &path) {
        Ok(Some(text)) => text,
        // Every member has one: cargo metadata listed it. A manifest gone since then fails cargo.
        Ok(None) => return Vec::new(),
        Err(unreadable) => return vec![unreadable],
    };
    let doc = match DeTable::parse(&text) {
        Ok(doc) => doc,
        Err(error) => return vec![not_toml(&path, &text, &error)],
    };
    let Some((key, value)) = entry(doc.get_ref(), "lints") else {
        return vec![report::lints_not_inherited(&path, 1, &member.name, false)];
    };
    let inherits = value
        .get_ref()
        .as_table()
        .and_then(|table| entry(table, "workspace"))
        .and_then(|(_, flag)| flag.get_ref().as_bool())
        == Some(true);
    if inherits {
        Vec::new()
    } else {
        let line = line_of(&text, key.span().start);
        vec![report::lints_not_inherited(&path, line, &member.name, true)]
    }
}

/// The violations for a `[workspace.lints.clippy]` entry of the root manifest that sets one of
/// `LINTS` to a level that does not report.
pub(super) fn workspace_lints(root: &Path) -> Vec<Violation> {
    let text = match checks::read_repo_file(root, ROOT_MANIFEST) {
        Ok(Some(text)) => text,
        Ok(None) => return Vec::new(),
        Err(unreadable) => return vec![unreadable],
    };
    let doc = match DeTable::parse(&text) {
        Ok(doc) => doc,
        Err(error) => return vec![not_toml(ROOT_MANIFEST, &text, &error)],
    };
    let clippy = table_at(doc.get_ref(), "workspace")
        .and_then(|workspace| table_at(workspace, "lints"))
        .and_then(|lints| table_at(lints, "clippy"));
    let mut found: Vec<Violation> = clippy
        .into_iter()
        .flat_map(|table| table.iter())
        .filter(|(name, _)| LINTS.contains(&lint_name(name.get_ref()).as_str()))
        .filter_map(|(name, value)| {
            let level = match value.get_ref() {
                DeValue::Table(table) => entry(table, "level")
                    .and_then(|(_, level)| level.get_ref().as_str())
                    .unwrap_or_default(),
                other => other.as_str().unwrap_or_default(),
            };
            (!REPORTING.contains(&level)).then(|| {
                let line = line_of(&text, name.span().start);
                report::workspace_lint_level(ROOT_MANIFEST, line, name.get_ref(), level)
            })
        })
        .collect();
    // The table iterates in key order; report in line order.
    found.sort_by_key(|violation| violation.line);
    found
}

/// The violations for each flag in the rustflags of `.cargo/config.toml` (`build.rustflags` and
/// every `target.<triple or cfg>.rustflags`) that names one of `LINTS`.
pub(super) fn cargo_config(root: &Path) -> Vec<Violation> {
    let text = match checks::read_repo_file(root, CARGO_CONFIG) {
        Ok(Some(text)) => text,
        Ok(None) => return Vec::new(),
        Err(unreadable) => return vec![unreadable],
    };
    let doc = match DeTable::parse(&text) {
        Ok(doc) => doc,
        Err(error) => return vec![not_toml(CARGO_CONFIG, &text, &error)],
    };
    let build = table_at(doc.get_ref(), "build").map(|build| ("build.rustflags".to_owned(), build));
    let targets = table_at(doc.get_ref(), "target")
        .into_iter()
        .flat_map(|targets| targets.iter())
        .filter_map(|(name, target)| {
            let table = target.get_ref().as_table()?;
            Some((format!("target.{}.rustflags", name.get_ref()), table))
        });
    let mut found = Vec::new();
    for (key, table) in build.into_iter().chain(targets) {
        let Some((_, flags)) = entry(table, "rustflags") else {
            continue;
        };
        for (flag, offset) in flags_of(flags) {
            let named = LINTS.iter().any(|lint| lint_name(&flag).contains(lint));
            if named {
                let line = line_of(&text, offset);
                found.push(report::rustflag_names_a_ban(
                    CARGO_CONFIG,
                    line,
                    &key,
                    &flag,
                ));
            }
        }
    }
    found
}

/// Each flag of a rustflags value, a string of space-separated flags or an array of them, with
/// the byte offset of the string it is in.
fn flags_of(value: &Spanned<DeValue<'_>>) -> Vec<(String, usize)> {
    let strings: Vec<(&str, usize)> = match value.get_ref() {
        DeValue::String(flags) => vec![(&**flags, value.span().start)],
        DeValue::Array(items) => items
            .iter()
            .filter_map(|item| Some((item.get_ref().as_str()?, item.span().start)))
            .collect(),
        _ => Vec::new(),
    };
    strings
        .into_iter()
        .flat_map(|(flags, offset)| {
            flags
                .split_whitespace()
                .map(move |flag| (flag.to_owned(), offset))
        })
        .collect()
}

/// `name` with each `-` read as `_`, as rustc reads a lint name.
fn lint_name(name: &str) -> String {
    name.replace('-', "_")
}

/// The violation for the TOML file `rel`, which does not parse.
fn not_toml(rel: &str, text: &str, error: &toml::de::Error) -> Violation {
    let line = error.span().map_or(1, |span| line_of(text, span.start));
    report::not_toml(rel, line, error)
}

/// The 1-based line of byte `offset` of `text`.
fn line_of(text: &str, offset: usize) -> u32 {
    super::clippy_toml::line_number(text, offset)
}

/// A key and its value, both with their spans.
type Entry<'s, 'a> = (&'s Spanned<DeString<'a>>, &'s Spanned<DeValue<'a>>);

/// The entry `key` of `table`.
fn entry<'s, 'a>(table: &'s DeTable<'a>, key: &str) -> Option<Entry<'s, 'a>> {
    table.iter().find(|(name, _)| name.get_ref() == key)
}

/// The table under `key` in `table`.
fn table_at<'s, 'a>(table: &'s DeTable<'a>, key: &str) -> Option<&'s DeTable<'a>> {
    entry(table, key).and_then(|(_, value)| value.get_ref().as_table())
}
