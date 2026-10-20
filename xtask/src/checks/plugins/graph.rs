//! The rule of the `plugins` check that reads the desktop crate's dependency graph
//! (`checks::graph`, the same per-crate `cargo tree` graph `dependencies` reads): no
//! `tauri-plugin-*` outside the fixed set is in it unless only fixed plugins pull it in.
//! `cargo metadata --no-deps` lists workspace members only, so a crate under `[workspace]
//! exclude` pulled in by path, or a registry or git dependency, shows up only here.
//! The resolved features (no `devtools` in tauri, tauri-runtime, tauri-runtime-wry or wry, no
//! `protocol-asset` in tauri) are banned in deny.toml's `[[bans.features]]`, which cargo-deny
//! checks for every target.

use super::FIXED_PLUGINS;
use crate::checks::graph::{self, Tree};
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
            found.push(violation_for(tree, id, dependent, desktop));
        }
    }
    found.sort_by(|a, b| (a.line, &a.message).cmp(&(b.line, &b.message)));
    found.dedup();
    found
}

/// The violation for `target`, a plugin reached through `dependent`. It points at the desktop
/// manifest line that declares the direct dependency responsible; when several lines declare a
/// node of it that brings the plugin in, it names them all, since removing only some of them may
/// leave the plugin in.
fn violation_for(tree: &Tree, target: usize, dependent: usize, desktop: &Desktop<'_>) -> Violation {
    let name = tree.name(target);
    let message = format!(
        "`{name}` {} is in the desktop crate's tree through `{}`, which is not a fixed plugin",
        tree.version(target),
        tree.name(dependent)
    );
    let blamed = graph::blame(tree, &responsible(tree, target), desktop.deps, desktop.text);
    match blamed.lines.as_slice() {
        [first, _, ..] => violation(
            desktop.path,
            *first,
            format!(
                "{message}. {}",
                blamed.declared(desktop.path, &format!("`{name}`"))
            ),
            format!(
                "remove {} from lines {} of {}: {TREE_FIX}",
                blamed.keys_text(),
                blamed.lines_text(),
                desktop.path
            ),
        ),
        lines => violation(
            desktop.path,
            lines.first().copied().unwrap_or(1),
            message,
            TREE_FIX,
        ),
    }
}

/// The direct dependencies of the desktop crate responsible for `target`. That is `target` itself
/// when the desktop crate depends on it directly, else the ones that reach it when they all have
/// one name, and none when no single name can be told apart.
fn responsible(tree: &Tree, target: usize) -> Vec<usize> {
    let direct = tree.deps(Tree::ROOT);
    if direct.contains(&target) {
        return vec![target];
    }
    let reaching: Vec<usize> = direct
        .iter()
        .copied()
        .filter(|&dep| tree.reach(dep).order().contains(&target))
        .collect();
    match reaching.first() {
        Some(&first)
            if reaching
                .iter()
                .all(|&dep| tree.name(dep) == tree.name(first)) =>
        {
            reaching
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
