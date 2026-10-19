//! The rule of the `plugins` check that reads the dependency graph cargo resolved, with deps: no
//! `tauri-plugin-*` outside the fixed set is in the desktop crate's tree unless only fixed plugins
//! pull it in. `cargo metadata --no-deps` lists workspace members only, so a crate under
//! `[workspace] exclude` pulled in by path, or a registry or git dependency, shows up only here.
//! The resolved features (no `devtools` in tauri, tauri-runtime, tauri-runtime-wry or wry, no
//! `protocol-asset` in tauri) are banned in deny.toml's `[[bans.features]]`, which cargo-deny
//! checks against the same graph for every target.

use super::FIXED_PLUGINS;
use crate::checks::toml_lines::{BUILT, Manifest};
use crate::checks::{self, violation};
use crate::violation::Violation;
use crate::workspace::{PackageDependencies, ResolvedGraph, ResolvedNode};
use std::collections::{BTreeMap, BTreeSet};

/// The plugin-authoring library every plugin depends on. It is not a plugin.
const PLUGIN_LIBRARY: &str = "tauri-plugin";

const TREE_FIX: &str =
    "only the fixed plugins and what they pull in may be in the desktop crate's tree";
pub(super) const GRAPH_FIX: &str = "make `cargo metadata --format-version 1 --locked --offline` succeed: run `cargo fetch --locked` when it fails on a missing download, else fix the manifests or the lockfile; this check reads the resolved graph from it";

/// Where the graph violations point: the desktop manifest and the desktop crate's dependencies
/// from `cargo metadata --no-deps`, which give the line of a direct dependency.
pub(super) struct Desktop<'a> {
    pub path: &'a str,
    pub text: &'a str,
    pub deps: &'a PackageDependencies,
}

/// Every violation in the resolved `graph`, located in the desktop manifest.
pub(super) fn violations(graph: &ResolvedGraph, desktop: &Desktop<'_>) -> Vec<Violation> {
    let tree = Tree::new(graph);
    let Some(root) = tree.desktop() else {
        return vec![violation(
            desktop.path,
            1,
            format!(
                "the resolved graph has no workspace member `{}`",
                checks::DESKTOP_PACKAGE
            ),
            GRAPH_FIX,
        )];
    };
    let mut found = plugin_violations(&tree, root, desktop);
    found.sort_by(|a, b| (a.line, &a.message).cmp(&(b.line, &b.message)));
    found.dedup();
    found
}

/// Each `tauri-plugin-*` outside the fixed set in the desktop crate's tree that a package other
/// than a fixed plugin depends on.
fn plugin_violations(tree: &Tree<'_>, root: &str, desktop: &Desktop<'_>) -> Vec<Violation> {
    let reachable = tree.reachable(root);
    let mut found = Vec::new();
    for &id in &reachable {
        let name = tree.name(id);
        let is_plugin = name.starts_with("tauri-plugin-") && name != PLUGIN_LIBRARY;
        if !is_plugin || FIXED_PLUGINS.contains(&name) {
            continue;
        }
        let outsider = reachable.iter().find(|&&dependent| {
            !FIXED_PLUGINS.contains(&tree.name(dependent))
                && tree.edges(dependent).any(|dep| dep == id)
        });
        if let Some(&dependent) = outsider {
            let version = tree
                .graph
                .packages
                .get(id)
                .map_or("", |p| p.version.as_str());
            found.push(violation(
                desktop.path,
                responsible_line(tree, root, id, desktop),
                format!(
                    "`{name}` {version} is in the desktop crate's tree through `{}`, which is not a fixed plugin",
                    tree.name(dependent)
                ),
                TREE_FIX,
            ));
        }
    }
    found
}

/// The desktop manifest line of the direct dependency responsible for `target`: `target` itself
/// when the desktop crate depends on it directly, else the one direct dependency that reaches
/// it. Line 1 when no single dependency can be told apart.
fn responsible_line(tree: &Tree<'_>, root: &str, target: &str, desktop: &Desktop<'_>) -> u32 {
    let direct: Vec<&str> = tree.edges(root).collect();
    let culprit = if direct.contains(&target) {
        Some(target)
    } else {
        let mut reaching = direct
            .iter()
            .filter(|&&dep| dep != root && tree.reachable(dep).contains(target));
        match (reaching.next(), reaching.next()) {
            (Some(&only), None) => Some(only),
            _ => None,
        }
    };
    let Some(culprit) = culprit else {
        return 1;
    };
    let name = tree.name(culprit);
    let Some(dep) = desktop.deps.dependencies.iter().find(|d| d.name == name) else {
        return 1;
    };
    Manifest::parse(desktop.text)
        .declaration(dep.key(), BUILT)
        .unwrap_or(1)
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

    /// The package id of the desktop crate among the workspace members.
    fn desktop(&self) -> Option<&'a str> {
        self.graph
            .members
            .iter()
            .map(String::as_str)
            .find(|id| self.name(id) == checks::DESKTOP_PACKAGE)
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

    /// `start` and every package id it reaches through `edges`.
    fn reachable(&self, start: &'a str) -> BTreeSet<&'a str> {
        let mut seen = BTreeSet::from([start]);
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            for dep in self.edges(id) {
                if seen.insert(dep) {
                    stack.push(dep);
                }
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests;
