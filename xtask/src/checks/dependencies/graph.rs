//! The graph walk of the `dependencies` check: is a package that a crate's row forbids anywhere
//! in that crate's own normal or build dependency graph?
//!
//! Each checked crate's graph comes from its own `cargo tree` run, read by `checks::graph`
//! (which says which edges count). The search starts from each direct dependency, so the
//! violation names the dependency that brings the forbidden package in. `cargo metadata
//! --no-deps` gives that dependency's key, renames and workspace inheritance resolved, so a
//! violation points at the line of the crate's `Cargo.toml` that declares it. Each declaration
//! is matched to the node it produced, and only the declarations whose node brings the package in
//! are blamed, each by its own key. When there are several, the violation names each line and
//! says what is known about them (`checks::graph::blame`).

use super::Fix;
use crate::checks::and_list;
use crate::checks::crates_table::{self, CrateRow};
use crate::checks::graph::{self, Tree};
use crate::checks::violation;
use crate::violation::Violation;
use crate::workspace::PackageDependencies;
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
                    nodes: BTreeSet::new(),
                });
            known.nodes.insert(direct);
            if known.path.len() > path.len() {
                known.path = path;
            }
        }
    }
    shortest
        .values()
        .map(|blame| violation_for(krate, tree, blame))
        .collect()
}

/// What one violation blames: a shortest path of package names from a direct dependency to a
/// forbidden package, and the direct dependencies of that name that reach it.
struct Blame<'a> {
    path: Vec<&'a str>,
    nodes: BTreeSet<usize>,
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
/// forbidden package in `tree`. It points at the first line that declares a node that brings the
/// package in and, when there are several such lines, names every one, since removing only some
/// of them may leave the path in place.
fn violation_for(krate: &Crate<'_>, tree: &Tree, blame: &Blame<'_>) -> Violation {
    let names = &blame.path;
    let package = names.last().copied().unwrap_or_default();
    let direct = names.first().copied().unwrap_or(package);
    let nodes: Vec<usize> = blame.nodes.iter().copied().collect();
    let blamed = graph::blame(tree, &nodes, krate.deps, krate.text);
    let quoted = if blamed.keys.is_empty() {
        format!("`{direct}`")
    } else {
        blamed.keys_text()
    };
    let lines = &blamed.lines;
    let through = match names.split_last() {
        Some((_, between)) if !between.is_empty() => {
            let between: Vec<String> = between.iter().map(|name| (*name).to_owned()).collect();
            format!(" through {}", and_list(&between))
        }
        _ => String::new(),
    };
    let (message, fix) = if let [_, _, ..] = lines.as_slice() {
        let at = blamed.lines_text();
        (
            format!(
                "{} depends on {package}{through}, which ARCHITECTURE.md forbids. {}",
                krate.package,
                blamed.declared(krate.manifest, package)
            ),
            Fix::MoveFromLines {
                keys: quoted,
                package: package.to_owned(),
                lines: at,
                manifest: krate.manifest.to_owned(),
                features: blamed.optional,
                row: krate.row.line,
                krate: krate.package.to_owned(),
            },
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
            Fix::MoveFromManifest {
                keys: quoted,
                package: package.to_owned(),
                manifest: krate.manifest.to_owned(),
                features: blamed.optional,
                row: krate.row.line,
                krate: krate.package.to_owned(),
            },
        )
    };
    violation(
        krate.manifest,
        lines.first().copied().unwrap_or(1),
        message,
        fix,
    )
}
