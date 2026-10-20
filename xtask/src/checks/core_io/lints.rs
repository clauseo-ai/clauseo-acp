//! The core-io rules on member manifests and the cargo config. `core_io.rs` states the rules.

use super::report;
use crate::checks;
use crate::violation::Violation;
use crate::workspace::Member;
use std::path::Path;
use toml::Spanned;
use toml::de::{DeString, DeTable, DeValue};

/// The external cargo commands xtask runs, in `steps/plan.rs` and `steps/policy.rs`. An `[alias]`
/// of the cargo config may not take their names, since cargo runs an alias in place of an
/// external command. It ignores an alias named after a built-in command such as `check`.
const XTASK_SUBCOMMANDS: [&str; 5] = ["clippy", "deny", "fmt", "nextest", "shear"];

/// The repo's cargo config.
pub(super) const CARGO_CONFIG: &str = ".cargo/config.toml";

/// The legacy name of the cargo config, which cargo reads instead of `CARGO_CONFIG`.
pub(super) const LEGACY_CARGO_CONFIG: &str = ".cargo/config";

/// The violation for the member `member` when its `Cargo.toml` does not say
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

/// The violations for the cargo config: a legacy `.cargo/config`, and in `.cargo/config.toml`
/// each `rustflags` key at any depth, each `[alias]` key named after an external cargo command
/// xtask runs and each `[env]` entry whose name starts with `CLIPPY_`.
pub(super) fn cargo_config(root: &Path) -> Vec<Violation> {
    let mut found = Vec::new();
    match checks::read_repo_file(root, LEGACY_CARGO_CONFIG) {
        Ok(Some(_)) => found.push(report::legacy_cargo_config(
            LEGACY_CARGO_CONFIG,
            CARGO_CONFIG,
        )),
        Ok(None) => {}
        Err(unreadable) => found.push(unreadable),
    }
    let text = match checks::read_repo_file(root, CARGO_CONFIG) {
        Ok(Some(text)) => text,
        Ok(None) => return found,
        Err(unreadable) => {
            found.push(unreadable);
            return found;
        }
    };
    let doc = match DeTable::parse(&text) {
        Ok(doc) => doc,
        Err(error) => {
            found.push(not_toml(CARGO_CONFIG, &text, &error));
            return found;
        }
    };
    let mut rustflags = Vec::new();
    rustflags_keys(doc.get_ref(), "", &mut rustflags);
    for (key, offset) in rustflags {
        let line = line_of(&text, offset);
        found.push(report::rustflags_in_config(CARGO_CONFIG, line, &key));
    }
    let aliases = table_at(doc.get_ref(), "alias")
        .into_iter()
        .flat_map(|alias| alias.iter());
    for (name, _) in
        aliases.filter(|(name, _)| XTASK_SUBCOMMANDS.contains(&name.get_ref().as_ref()))
    {
        let line = line_of(&text, name.span().start);
        found.push(report::alias_shadows_command(
            CARGO_CONFIG,
            line,
            name.get_ref(),
        ));
    }
    let env = table_at(doc.get_ref(), "env")
        .into_iter()
        .flat_map(|env| env.iter());
    for (name, _) in env.filter(|(name, _)| is_clippy_env(name.get_ref())) {
        let line = line_of(&text, name.span().start);
        found.push(report::clippy_env_in_config(
            CARGO_CONFIG,
            line,
            name.get_ref(),
        ));
    }
    found.sort_by_key(|violation| (violation.path != LEGACY_CARGO_CONFIG, violation.line));
    found
}

/// Pushes onto `found` the dotted path and key offset of each `rustflags` key in `table` (whose
/// own path is `prefix`) and in every table under it.
fn rustflags_keys(table: &DeTable<'_>, prefix: &str, found: &mut Vec<(String, usize)>) {
    for (name, value) in table {
        let path = if prefix.is_empty() {
            name.get_ref().to_string()
        } else {
            format!("{prefix}.{}", name.get_ref())
        };
        if name.get_ref() == "rustflags" {
            found.push((path.clone(), name.span().start));
        }
        if let Some(inner) = value.get_ref().as_table() {
            rustflags_keys(inner, &path, found);
        }
    }
}

/// True when the `[env]` entry `name` starts with `CLIPPY_` in any ASCII case: Windows reads
/// environment names ignoring case, so `Clippy_Conf_Dir` there is `CLIPPY_CONF_DIR`.
fn is_clippy_env(name: &str) -> bool {
    name.get(.."CLIPPY_".len())
        .is_some_and(|head| head.eq_ignore_ascii_case("CLIPPY_"))
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
