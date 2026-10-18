//! The rules of the `plugins` check that read the dependency graph cargo resolved, with deps:
//! no Tauri crate is built with `devtools` (nor `tauri` with `protocol-asset`), and no
//! `tauri-plugin-*` outside the fixed set is in the desktop crate's tree unless only fixed
//! plugins pull it in. `cargo metadata --no-deps` lists workspace members only, so a crate under
//! `[workspace] exclude` pulled in by path, or a registry or git dependency that turns those
//! features on, shows up only here.

use super::FIXED_PLUGINS;
use super::toml_lines::{declaration, lines};
use crate::checks::{self, violation};
use crate::violation::Violation;
use crate::workspace::{PackageDependencies, ResolvedGraph, ResolvedNode};
use std::collections::{BTreeMap, BTreeSet};

/// The crates whose resolved features must not include `devtools`.
const DEVTOOLS_CRATES: [&str; 4] = ["tauri", "tauri-runtime", "tauri-runtime-wry", "wry"];

/// The plugin-authoring library every plugin depends on. It is not a plugin.
const PLUGIN_LIBRARY: &str = "tauri-plugin";

const DEVTOOLS_WHY: &str = "devtools let script in the page open the inspector";
const ASSET_WHY: &str = "`protocol-asset` serves files from disk to the webview";
const TREE_FIX: &str =
    "only the fixed plugins and what they pull in may be in the desktop crate's tree";
pub(super) const GRAPH_FIX: &str = "fix the manifests or the lockfile until `cargo metadata --format-version 1 --locked` succeeds: this check reads the resolved features from it";

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
    let mut found = feature_violations(&tree, root, desktop);
    found.extend(plugin_violations(&tree, root, desktop));
    found.sort_by(|a, b| (a.line, &a.message).cmp(&(b.line, &b.message)));
    found.dedup();
    found
}

/// `devtools` in the resolved features of a `DEVTOOLS_CRATES` node, and `protocol-asset` in
/// `tauri`'s, anywhere in the graph.
fn feature_violations(tree: &Tree<'_>, root: &str, desktop: &Desktop<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    for node in &tree.graph.nodes {
        let name = tree.name(&node.id);
        if !DEVTOOLS_CRATES.contains(&name) {
            continue;
        }
        let enabled = [
            ("devtools", has(node, "devtools"), DEVTOOLS_WHY),
            (
                "protocol-asset",
                name == "tauri" && has(node, "protocol-asset"),
                ASSET_WHY,
            ),
        ];
        for (feature, on, why) in enabled {
            if !on {
                continue;
            }
            found.push(violation(
                desktop.path,
                responsible_line(tree, root, &node.id, desktop),
                format!("`{name}` is built with its `{feature}` feature (resolved by cargo)"),
                format!(
                    "find what enables it with `cargo tree -e features -i {name}` and remove it: {why}"
                ),
            ));
        }
    }
    found
}

/// True when `feature` is among the resolved features of `node`.
fn has(node: &ResolvedNode, feature: &str) -> bool {
    node.features.iter().any(|f| f == feature)
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
    let in_table = |header: &str| {
        header == "[dependencies]"
            || header == "[build-dependencies]"
            || (header.starts_with("[target.")
                && (header.ends_with(".dependencies]") || header.ends_with(".build-dependencies]")))
    };
    declaration(&lines(desktop.text), dep.key(), in_table).map_or(1, |line| line.no)
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
