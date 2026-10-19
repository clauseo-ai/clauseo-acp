//! The graph walk of the `dependencies` check: is a package that a crate's row forbids anywhere in
//! that crate's normal or build dependency graph?
//!
//! The graph comes from one `cargo metadata --format-version 1 --all-features` run with deps:
//! all features, so an optional dependency counts; no `--filter-platform`, so every target's
//! dependencies count. Its `resolve.nodes[].deps[].dep_kinds` give the edges, and only normal and
//! build edges are followed (dev-dependencies never reach the built crate). The same run's
//! `packages[].dependencies` give each direct dependency's key, renames and workspace inheritance
//! resolved, so a violation points at the line of the crate's `Cargo.toml` that declares it.

use super::and_list;
use crate::checks::crates_table::{self, CrateRow};
use crate::checks::toml_lines::{BUILT, Manifest};
use crate::checks::violation;
use crate::process::{self, Cmd, Program};
use crate::violation::Violation;
use crate::workspace::{self, PackageDependencies, ResolvedGraph, ResolvedNode};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

/// The root manifest, where a failure to read the graph points.
const ROOT_MANIFEST: &str = "Cargo.toml";

/// The resolved graph and the declared dependencies of every package, from one metadata run.
pub(super) struct Graph {
    resolved: ResolvedGraph,
    declared: BTreeMap<String, PackageDependencies>,
}

/// One workspace crate to walk from, with its manifest and its row.
pub(super) struct Crate<'a> {
    pub package: &'a str,
    /// The repo-relative path of its `Cargo.toml`.
    pub manifest: &'a str,
    /// The text of that manifest, read only for line numbers.
    pub text: &'a str,
    pub row: &'a CrateRow,
}

impl Graph {
    /// Runs `cargo metadata --format-version 1 --all-features <flags>` in `root`. `Err` is the
    /// violation to report, at `Cargo.toml:1`, with a fix for the cause cargo gave.
    pub(super) fn load(root: &Path, flags: &[&str]) -> Result<Graph, Violation> {
        let mut args = vec!["metadata", "--format-version", "1", "--all-features"];
        args.extend_from_slice(flags);
        let cmd = Cmd::new(Program::Cargo, args);
        let failed = |message: String, fix: &str| violation(ROOT_MANIFEST, 1, message, fix);
        let quiet = process::run_quiet(&cmd, root).map_err(|error| {
            failed(
                format!("could not run `{cmd}`: {error}"),
                "put `cargo` on PATH: this check reads the dependency graph from `cargo metadata`",
            )
        })?;
        if !quiet.success {
            let stderr = quiet.stderr.trim();
            return Err(failed(
                format!("`{cmd}` failed: {stderr}"),
                failure_fix(stderr),
            ));
        }
        let bad_json = |error: String| {
            failed(
                format!("could not read the output of `{cmd}`: {error}"),
                "update xtask/src/workspace.rs to the JSON this cargo writes: this check reads the dependency graph from it",
            )
        };
        Ok(Graph {
            resolved: workspace::parse_resolved_graph(&quiet.stdout).map_err(bad_json)?,
            declared: workspace::parse_workspace_dependencies(&quiet.stdout).map_err(bad_json)?,
        })
    }

    /// One violation per direct dependency of `krate` and package of its row's Never list that
    /// the dependency is or brings in.
    pub(super) fn violations(&self, krate: &Crate<'_>) -> Vec<Violation> {
        let mut forbidden: Vec<String> = krate
            .row
            .never
            .iter()
            .filter_map(crates_table::Never::package)
            .map(|package| fold(&package))
            .collect();
        forbidden.sort();
        forbidden.dedup();
        if forbidden.is_empty() {
            return Vec::new();
        }
        let tree = Tree::new(&self.resolved);
        let Some(root) = tree.member(krate.package) else {
            return vec![violation(
                krate.manifest,
                1,
                format!(
                    "`cargo metadata` lists no workspace member `{}`",
                    krate.package
                ),
                "add the crate to `[workspace] members` in Cargo.toml: this check reads its dependency graph from `cargo metadata`",
            )];
        };
        let direct: BTreeSet<&str> = tree.edges(root).collect();
        let mut found = Vec::new();
        for dep in direct {
            for path in tree.first_paths(dep, &forbidden) {
                found.push(self.violation(krate, &tree, &path));
            }
        }
        found
    }

    /// The violation for `path`, from a direct dependency of `krate` to a forbidden package.
    fn violation(&self, krate: &Crate<'_>, tree: &Tree<'_>, path: &[&str]) -> Violation {
        let names: Vec<String> = path.iter().map(|id| tree.name(id).to_owned()).collect();
        let package = names.last().map_or("", String::as_str);
        let direct = names.first().map_or(package, String::as_str);
        let entries: Vec<_> = self
            .declared
            .get(krate.package)
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
            Some((_, between)) if !between.is_empty() => format!(" through {}", and_list(between)),
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
}

/// The fix for a failed metadata run whose stderr is `stderr`, true for the cause cargo names.
pub(super) fn failure_fix(stderr: &str) -> &'static str {
    if stderr.contains("--locked was passed") {
        "Cargo.lock does not match the manifests and the check never rewrites it: run `cargo update --workspace` to bring it in line, then commit Cargo.lock"
    } else if stderr.contains("--offline") || stderr.contains("offline mode") {
        "a crate is not downloaded and the check runs offline: run `cargo fetch --locked`, then run the check again"
    } else {
        "fix what cargo reports above until `cargo metadata` succeeds: this check reads the dependency graph from it"
    }
}

/// The resolved graph indexed by package id, following normal and build edges for every target.
struct Tree<'a> {
    graph: &'a ResolvedGraph,
    nodes: BTreeMap<&'a str, &'a ResolvedNode>,
}

impl<'a> Tree<'a> {
    fn new(graph: &'a ResolvedGraph) -> Self {
        Tree {
            graph,
            nodes: graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect(),
        }
    }

    /// The package id of the workspace member `package`.
    fn member(&self, package: &str) -> Option<&'a str> {
        self.graph
            .members
            .iter()
            .map(String::as_str)
            .find(|id| self.name(id) == package)
    }

    /// The package name of `id`, or `""` when the graph does not list it.
    fn name(&self, id: &str) -> &'a str {
        self.graph.packages.get(id).map_or("", |p| p.name.as_str())
    }

    /// The package ids `id` depends on through a normal or build edge, for any target.
    fn edges(&self, id: &str) -> impl Iterator<Item = &'a str> + use<'a> {
        self.nodes
            .get(id)
            .map(|node| node.deps.as_slice())
            .unwrap_or_default()
            .iter()
            .filter(|dep| {
                dep.dep_kinds
                    .iter()
                    .any(|k| matches!(k.kind.as_deref(), None | Some("normal" | "build")))
            })
            .map(|dep| dep.pkg.as_str())
    }

    /// For each package of `wanted` (names folded, see `fold`) that `start` is or reaches, a
    /// shortest path from `start` to it (both ends included), found breadth first.
    fn first_paths(&self, start: &'a str, wanted: &[String]) -> Vec<Vec<&'a str>> {
        let mut parent: BTreeMap<&'a str, &'a str> = BTreeMap::new();
        let mut seen = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        let mut done: BTreeSet<&str> = BTreeSet::new();
        let mut found: Vec<Vec<&'a str>> = Vec::new();
        while let Some(id) = queue.pop_front() {
            let name = fold(self.name(id));
            if let Some(package) = wanted.iter().find(|package| **package == name)
                && done.insert(package.as_str())
            {
                let mut path = vec![id];
                let mut at = id;
                while let Some(&up) = parent.get(at) {
                    path.push(up);
                    at = up;
                }
                path.reverse();
                found.push(path);
            }
            for dep in self.edges(id) {
                if seen.insert(dep) {
                    parent.insert(dep, id);
                    queue.push_back(dep);
                }
            }
        }
        found
    }
}

/// `name` with every `-` written `_`: cargo treats the two as the same in a package name, so a
/// ban on `transport_ws` or `async_process` matches `clauseo-transport-ws` or `async-process`.
fn fold(name: &str) -> String {
    name.replace('-', "_")
}
