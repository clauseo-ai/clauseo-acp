//! The `rust-version` rule: the workspace manifest's `rust-version` (in `[workspace.package]` or
//! `[package]`) and each member's own `[package] rust-version` have the major.minor of `channel`
//! in `rust-toolchain.toml`. A member that inherits it (`rust-version.workspace = true`) holds no
//! copy. Spans come from `toml`'s `DeTable`, so a dotted key or a quoted key is found too.

use super::{CARGO_TOML, Fix, TOOLCHAIN, major_minor};
use crate::checks::{self, Inspection};
use crate::workspace::Context;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

/// Reads the root manifest and each member's, comparing each `rust-version` with `channel`
/// (`None` when the home could not be read: then it only counts the manifests).
pub(super) fn check(ctx: &Context, channel: Option<&str>, found: &mut Inspection) {
    let mut manifests = vec![CARGO_TOML.to_owned()];
    // When `cargo metadata` failed, the steps before the checks already print why; the root
    // manifest, which holds the copy in this repo, is still read.
    for member in ctx.members.as_deref().unwrap_or_default() {
        let manifest = checks::join(&member.dir, CARGO_TOML);
        if !manifests.contains(&manifest) {
            manifests.push(manifest);
        }
    }
    for manifest in manifests {
        match checks::read_repo_file(&ctx.root, &manifest) {
            Ok(None) => {}
            Ok(Some(text)) => {
                found.inspected = found.inspected.saturating_add(1);
                compare(&manifest, &text, channel, found);
            }
            Err(unreadable) => {
                found.inspected = found.inspected.saturating_add(1);
                found.violations.push(unreadable);
            }
        }
    }
}

fn compare(manifest: &str, text: &str, channel: Option<&str>, found: &mut Inspection) {
    let doc = match DeTable::parse(text) {
        Ok(doc) => doc,
        Err(error) => {
            let at = error.span().map_or(0, |span| span.start);
            found.violations.push(checks::violation(
                manifest,
                line(text, at),
                format!("{manifest} is not TOML: {}", error.message()),
                Fix::NotToml,
            ));
            return;
        }
    };
    let Some(channel) = channel else {
        return;
    };
    let wanted = major_minor(channel);
    let root = doc.get_ref();
    let workspace = table(root, "workspace").and_then(|workspace| table(workspace, "package"));
    for package in [workspace, table(root, "package")].into_iter().flatten() {
        let Some(value) = entry(package, "rust-version") else {
            continue;
        };
        let Some(version) = value.get_ref().as_str() else {
            continue;
        };
        if major_minor(version) != wanted {
            found.violations.push(checks::violation(
                manifest,
                line(text, value.span().start),
                format!(
                    "rust-version \"{version}\" is not the major.minor of the Rust version \
                     `channel` in {TOOLCHAIN} pins ({channel})"
                ),
                Fix::RustVersion {
                    channel_major_minor: wanted.to_owned(),
                },
            ));
        }
    }
}

/// The value of `key` in `table`.
fn entry<'s, 'a>(table: &'s DeTable<'a>, key: &str) -> Option<&'s Spanned<DeValue<'a>>> {
    table
        .iter()
        .find(|(name, _)| name.get_ref() == key)
        .map(|(_, value)| value)
}

/// The table under `key` in `table`.
fn table<'s, 'a>(table: &'s DeTable<'a>, key: &str) -> Option<&'s DeTable<'a>> {
    entry(table, key).and_then(|value| value.get_ref().as_table())
}

/// The line, from 1, of byte `offset` in `text`.
fn line(text: &str, offset: usize) -> u32 {
    let before = text.get(..offset).unwrap_or(text);
    checks::line_at(before.matches('\n').count())
}
