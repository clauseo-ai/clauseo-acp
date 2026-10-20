//! The graph walk of the `dependencies` check: is a package that a crate's row forbids anywhere
//! in that crate's own normal or build dependency graph?
//!
//! Each checked crate's graph comes from its own `cargo tree` run, read by `checks::graph`
//! (which says which edges count). The search starts from each direct dependency, so the
//! violation names the dependency that brings the forbidden package in. `cargo metadata
//! --no-deps` gives that dependency's key, renames and workspace inheritance resolved, so a
//! violation points at the line of the crate's `Cargo.toml` that declares it. A direct dependency
//! declared in both `[dependencies]` and `[build-dependencies]` is blamed on the line of each
//! section whose node brings the package in (`Tree::sections`), and names each of those lines.

use crate::checks::and_list;
use crate::checks::crates_table::{self, CrateRow};
use crate::checks::graph::{Section, Tree};
use crate::checks::toml_lines::Manifest;
use crate::checks::violation;
use crate::violation::Violation;
use crate::workspace::{CargoDependency, PackageDependencies};
use std::collections::{BTreeMap, BTreeSet};

/// One workspace crate to walk from, with its manifest, its row and its declared dependencies.
pub(super) struct Crate<'a> {
    pub package: &'a str,
    /// The repo-relative path of its `Cargo.toml`.
    pub manifest: &'a str,
    /// The text of that manifest, read only for line numbers.
    pub text: &'a str,
    pub row: &'a CrateRow,
    /// Its dependencies from `cargo metadata --no-deps`.
    pub deps: &'a PackageDependencies,
}

/// One violation per direct dependency of `krate` and package of its row's Never list that the
/// dependency is or brings in, for the crate's graph `tree`, through a shortest path (the first
/// in `cargo tree`'s order among equals). Direct dependencies with one name count together.
pub(super) fn violations(krate: &Crate<'_>, tree: &Tree) -> Vec<Violation> {
    let forbidden = forbidden(krate.row);
    let mut shortest: BTreeMap<(&str, String), Blame<'_>> = BTreeMap::new();
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
                .or_insert_with(|| Blame {
                    path: path.clone(),
                    sections: BTreeSet::new(),
                });
            known.sections.extend(tree.sections(direct));
            if known.path.len() > path.len() {
                known.path = path;
            }
        }
    }
    shortest
        .values()
        .map(|blame| violation_for(krate, blame))
        .collect()
}

/// What one violation blames: a shortest path of package names from a direct dependency to a
/// forbidden package, and the sections of the direct dependencies of that name that reach it.
struct Blame<'a> {
    path: Vec<&'a str>,
    sections: BTreeSet<Section>,
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

/// The violation for `blame`, a path of package names from a direct dependency of `krate` to a
/// forbidden package. The lines that declare the direct dependency in the sections that bring the
/// package in are all responsible: the violation points at the first and, when there are
/// several, names every one, since removing a single line would leave the path in place.
fn violation_for(krate: &Crate<'_>, blame: &Blame<'_>) -> Violation {
    let names = &blame.path;
    let package = names.last().copied().unwrap_or_default();
    let direct = names.first().copied().unwrap_or(package);
    let manifest = Manifest::parse(krate.text);
    let entries: Vec<(&CargoDependency, Section)> = krate
        .deps
        .dependencies
        .iter()
        .filter(|dep| dep.name == direct)
        .filter_map(|dep| Some((dep, Section::of_kind(dep.kind.as_deref())?)))
        .filter(|(_, section)| blame.sections.contains(section))
        .collect();
    let mut keys: Vec<String> = entries
        .iter()
        .map(|(dep, _)| dep.key().to_owned())
        .collect();
    keys.sort();
    keys.dedup();
    if keys.is_empty() {
        keys.push(direct.to_owned());
    }
    let mut lines: Vec<u32> = entries
        .iter()
        .flat_map(|(dep, section)| manifest.declarations(dep.key(), section.tables()))
        .collect();
    lines.sort_unstable();
    lines.dedup();
    let through = match names.split_last() {
        Some((_, between)) if !between.is_empty() => {
            let between: Vec<String> = between.iter().map(|name| (*name).to_owned()).collect();
            format!(" through {}", and_list(&between))
        }
        _ => String::new(),
    };
    let quoted: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
    let quoted = and_list(&quoted);
    let features = if entries.iter().any(|(dep, _)| dep.optional) {
        " and every `[features]` item that enables it"
    } else {
        ""
    };
    let (message, fix) = if let [_, _, ..] = lines.as_slice() {
        let at: Vec<String> = lines.iter().map(u32::to_string).collect();
        let at = and_list(&at);
        (
            format!(
                "{} depends on {package}{through}, which ARCHITECTURE.md forbids: {quoted} is declared at lines {at} of {}, and each declaration brings {package} in",
                krate.package, krate.manifest
            ),
            format!(
                "move the code that uses {quoted} into a crate whose row allows {package}, then remove {quoted} from lines {at} of {manifest}{features}: the row at ARCHITECTURE.md:{row} says {krate} never depends on {package}",
                manifest = krate.manifest,
                row = krate.row.line,
                krate = krate.package,
            ),
        )
    } else {
        let unlocated = if lines.is_empty() {
            format!(
                " (xtask found no line declaring {quoted} in {}, so this points at line 1)",
                krate.manifest
            )
        } else {
            String::new()
        };
        (
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
    };
    violation(
        krate.manifest,
        lines.first().copied().unwrap_or(1),
        message,
        fix,
    )
}
