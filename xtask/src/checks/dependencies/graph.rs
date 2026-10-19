//! The graph walk of the `dependencies` check: is a package that a crate's row forbids anywhere
//! in that crate's own normal or build dependency graph?
//!
//! Each checked crate's graph comes from its own `cargo tree` run, read by `checks::graph`
//! (which says which edges count). The search starts from each direct dependency, so the
//! violation names the dependency that brings the forbidden package in. `cargo metadata
//! --no-deps` gives that dependency's key, renames and workspace inheritance resolved, so a
//! violation points at the line of the crate's `Cargo.toml` that declares it.

use super::and_list;
use crate::checks::crates_table::{self, CrateRow};
use crate::checks::graph::Tree;
use crate::checks::toml_lines::{BUILT, Manifest};
use crate::checks::violation;
use crate::violation::Violation;
use crate::workspace::PackageDependencies;
use std::collections::BTreeMap;

/// One workspace crate to walk from, with its manifest, its row and its declared dependencies.
pub(super) struct Crate<'a> {
    pub package: &'a str,
    /// The repo-relative path of its `Cargo.toml`.
    pub manifest: &'a str,
    /// The text of that manifest, read only for line numbers.
    pub text: &'a str,
    pub row: &'a CrateRow,
    /// Its dependencies from `cargo metadata --no-deps`, `None` when cargo listed none for it.
    pub deps: Option<&'a PackageDependencies>,
}

/// One violation per direct dependency of `krate` and package of its row's Never list that the
/// dependency is or brings in, for the crate's graph `tree`, through a shortest path (the first
/// in `cargo tree`'s order among equals).
pub(super) fn violations(krate: &Crate<'_>, tree: &Tree) -> Vec<Violation> {
    let forbidden = forbidden(krate.row);
    let mut shortest: BTreeMap<(&str, String), Vec<&str>> = BTreeMap::new();
    for &direct in tree.deps(Tree::ROOT) {
        let reach = tree.reach(direct);
        for &package in reach.order() {
            let folded = crates_table::fold(tree.name(package));
            if !forbidden.contains(&folded) {
                continue;
            }
            let path: Vec<&str> = reach
                .path(package)
                .into_iter()
                .map(|id| tree.name(id))
                .collect();
            let known = shortest
                .entry((tree.name(direct), folded))
                .or_insert_with(|| path.clone());
            if known.len() > path.len() {
                *known = path;
            }
        }
    }
    shortest
        .values()
        .map(|path| violation_for(krate, path))
        .collect()
}

/// The packages `row` bans, names folded (see `crates_table::fold`), sorted, each once.
pub(super) fn forbidden(row: &CrateRow) -> Vec<String> {
    let mut forbidden: Vec<String> = row
        .never
        .iter()
        .filter_map(crates_table::Never::package)
        .map(|package| crates_table::fold(&package))
        .collect();
    forbidden.sort();
    forbidden.dedup();
    forbidden
}

/// The violation for `names`, a path of package names from a direct dependency of `krate` to a
/// forbidden package.
fn violation_for(krate: &Crate<'_>, names: &[&str]) -> Violation {
    let package = names.last().copied().unwrap_or_default();
    let direct = names.first().copied().unwrap_or(package);
    let entries: Vec<_> = krate
        .deps
        .map(|deps| deps.dependencies.as_slice())
        .unwrap_or_default()
        .iter()
        .filter(|dep| dep.name == direct && !matches!(dep.kind.as_deref(), Some("dev")))
        .collect();
    let mut keys: Vec<String> = entries.iter().map(|dep| dep.key().to_owned()).collect();
    keys.sort();
    keys.dedup();
    if keys.is_empty() {
        keys.push(direct.to_owned());
    }
    let manifest = Manifest::parse(krate.text);
    let declared = keys
        .iter()
        .filter_map(|key| manifest.declaration(key, BUILT))
        .min();
    let through = match names.split_last() {
        Some((_, between)) if !between.is_empty() => {
            let between: Vec<String> = between.iter().map(|name| (*name).to_owned()).collect();
            format!(" through {}", and_list(&between))
        }
        _ => String::new(),
    };
    let quoted: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
    let quoted = and_list(&quoted);
    let unlocated = if declared.is_none() {
        format!(
            " (xtask found no line declaring {quoted} in {}, so this points at line 1)",
            krate.manifest
        )
    } else {
        String::new()
    };
    let features = if entries.iter().any(|dep| dep.optional) {
        " and every `[features]` item that enables it"
    } else {
        ""
    };
    violation(
        krate.manifest,
        declared.unwrap_or(1),
        format!(
            "{} depends on {package}{through}, which ARCHITECTURE.md forbids{unlocated}",
            krate.package
        ),
        format!(
            "move the code that uses {quoted} into a crate whose row allows {package}, then remove {quoted}{features} from {manifest}: the row at ARCHITECTURE.md:{row} says {krate} never depends on {package}",
            manifest = krate.manifest,
            row = krate.row.line,
            krate = krate.package,
        ),
    )
}
