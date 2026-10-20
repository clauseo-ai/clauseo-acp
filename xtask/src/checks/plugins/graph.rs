//! The rule of the `plugins` check that reads the desktop crate's dependency graph
//! (`checks::graph`, the same per-crate `cargo tree` graph `dependencies` reads): no
//! `tauri-plugin-*` outside the fixed set is in it unless only fixed plugins pull it in.
//! `cargo metadata --no-deps` lists workspace members only, so a crate under `[workspace]
//! exclude` pulled in by path, or a registry or git dependency, shows up only here.
//! The resolved features (no `devtools` in tauri, tauri-runtime, tauri-runtime-wry or wry, no
//! `protocol-asset` in tauri) are banned in deny.toml's `[[bans.features]]`, which cargo-deny
//! checks for every target.

use super::FIXED_PLUGINS;
use crate::checks::and_list;
use crate::checks::graph::{Section, Tree};
use crate::checks::toml_lines::Manifest;
use crate::checks::violation;
use crate::violation::Violation;
use crate::workspace::PackageDependencies;
use std::collections::BTreeSet;

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
/// manifest line that declares the direct dependency responsible; when that dependency is
/// declared on several lines, it names them all, since removing one would leave the plugin in.
fn violation_for(tree: &Tree, target: usize, dependent: usize, desktop: &Desktop<'_>) -> Violation {
    let name = tree.name(target);
    let message = format!(
        "`{name}` {} is in the desktop crate's tree through `{}`, which is not a fixed plugin",
        tree.version(target),
        tree.name(dependent)
    );
    let (culprit, lines) = responsible(tree, target, desktop);
    match lines.as_slice() {
        [_, _, ..] => {
            let at: Vec<String> = lines.iter().map(u32::to_string).collect();
            let at = and_list(&at);
            violation(
                desktop.path,
                lines[0],
                format!(
                    "{message}: `{culprit}` is declared at lines {at} of {}, and each declaration brings `{name}` in",
                    desktop.path
                ),
                format!(
                    "remove `{culprit}` from lines {at} of {}: {TREE_FIX}",
                    desktop.path
                ),
            )
        }
        _ => violation(
            desktop.path,
            lines.first().copied().unwrap_or(1),
            message,
            TREE_FIX,
        ),
    }
}

/// The direct dependency responsible for `target`, its name, and the desktop manifest lines that
/// declare it in the sections its node came under, in order. `target` itself when the desktop
/// crate depends on it directly, else the one direct dependency name that reaches it. No lines
/// when no single name can be told apart, or when no declaration is found.
fn responsible(tree: &Tree, target: usize, desktop: &Desktop<'_>) -> (String, Vec<u32>) {
    let direct = tree.deps(Tree::ROOT);
    let culprit: Option<(&str, Vec<Section>)> = if direct.contains(&target) {
        Some((tree.name(target), tree.sections(target).to_vec()))
    } else {
        let reaching: Vec<usize> = direct
            .iter()
            .copied()
            .filter(|&dep| tree.reach(dep).order().contains(&target))
            .collect();
        match reaching.as_slice() {
            [first, ..]
                if reaching
                    .iter()
                    .all(|&dep| tree.name(dep) == tree.name(*first)) =>
            {
                let sections: BTreeSet<Section> = reaching
                    .iter()
                    .flat_map(|&dep| tree.sections(dep).iter().copied())
                    .collect();
                Some((tree.name(*first), sections.into_iter().collect()))
            }
            _ => None,
        }
    };
    let Some((name, sections)) = culprit else {
        return (String::new(), Vec::new());
    };
    let manifest = Manifest::parse(desktop.text);
    let mut lines: Vec<u32> = desktop
        .deps
        .dependencies
        .iter()
        .filter(|dep| dep.name == name)
        .filter_map(|dep| Some((dep, Section::of_kind(dep.kind.as_deref())?)))
        .filter(|(_, section)| sections.contains(section))
        .flat_map(|(dep, section)| manifest.declarations(dep.key(), section.tables()))
        .collect();
    lines.sort_unstable();
    lines.dedup();
    (name.to_owned(), lines)
}

#[cfg(test)]
mod tests;
