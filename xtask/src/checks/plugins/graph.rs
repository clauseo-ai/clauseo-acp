//! The rule of the `plugins` check that reads the desktop crate's dependency graph
//! (`checks::graph`, the same per-crate `cargo tree` graph `dependencies` reads): no
//! `tauri-plugin-*` outside the fixed set is in it unless only fixed plugins pull it in.
//! `cargo metadata --no-deps` lists workspace members only, so a crate under `[workspace]
//! exclude` pulled in by path, or a registry or git dependency, shows up only here.
//! The resolved features (no `devtools` in tauri, tauri-runtime, tauri-runtime-wry or wry, no
//! `protocol-asset` in tauri) are banned in deny.toml's `[[bans.features]]`, which cargo-deny
//! checks for every target.

use super::FIXED_PLUGINS;
use crate::checks::graph::Tree;
use crate::checks::toml_lines::{BUILT, Manifest};
use crate::checks::violation;
use crate::violation::Violation;
use crate::workspace::PackageDependencies;

/// The plugin-authoring library every plugin depends on. It is not a plugin.
const PLUGIN_LIBRARY: &str = "tauri-plugin";

const TREE_FIX: &str =
    "only the fixed plugins and what they pull in may be in the desktop crate's tree";

/// Where the graph violations point: the desktop manifest and the desktop crate's dependencies
/// from `cargo metadata --no-deps`, which give the line of a direct dependency.
pub(super) struct Desktop<'a> {
    pub path: &'a str,
    pub text: &'a str,
    pub deps: &'a PackageDependencies,
}

/// Every violation in `tree`, the desktop crate's graph, located in the desktop manifest: each
/// `tauri-plugin-*` outside the fixed set that a package other than a fixed plugin depends on.
pub(super) fn violations(tree: &Tree, desktop: &Desktop<'_>) -> Vec<Violation> {
    let reach = tree.reach(Tree::ROOT);
    let reachable = reach.order();
    let mut found = Vec::new();
    for &id in reachable {
        let name = tree.name(id);
        let is_plugin = name.starts_with("tauri-plugin-") && name != PLUGIN_LIBRARY;
        if !is_plugin || FIXED_PLUGINS.contains(&name) {
            continue;
        }
        let outsider = reachable.iter().find(|&&dependent| {
            !FIXED_PLUGINS.contains(&tree.name(dependent)) && tree.deps(dependent).contains(&id)
        });
        if let Some(&dependent) = outsider {
            found.push(violation(
                desktop.path,
                responsible_line(tree, id, desktop),
                format!(
                    "`{name}` {} is in the desktop crate's tree through `{}`, which is not a fixed plugin",
                    tree.version(id),
                    tree.name(dependent)
                ),
                TREE_FIX,
            ));
        }
    }
    found.sort_by(|a, b| (a.line, &a.message).cmp(&(b.line, &b.message)));
    found.dedup();
    found
}

/// The desktop manifest line of the direct dependency responsible for `target`: `target` itself
/// when the desktop crate depends on it directly, else the one direct dependency that reaches
/// it. Line 1 when no single dependency can be told apart.
fn responsible_line(tree: &Tree, target: usize, desktop: &Desktop<'_>) -> u32 {
    let direct = tree.deps(Tree::ROOT);
    let culprit = if direct.contains(&target) {
        Some(target)
    } else {
        let mut reaching = direct
            .iter()
            .filter(|&&dep| tree.reach(dep).order().contains(&target));
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

#[cfg(test)]
mod tests;
