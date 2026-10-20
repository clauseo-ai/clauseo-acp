//! The `plugins` check: the cargo side of Tauri in the workspace. The desktop crate has the fixed
//! plugin set and safe `tauri` and `tauri-runtime` features, and no other member depends on Tauri.
//!
//! The Tauri family is `wry`, `tao` and every package whose name starts with `tauri`.
//!
//! The dependencies of every member come from the one `cargo metadata --no-deps` run that
//! `workspace::load` makes, which resolves workspace inheritance and renames, so a renamed or
//! inherited entry counts under its real package. Among normal dependencies, with or without a
//! target:
//! - in the desktop crate, the `tauri-plugin-*` packages are exactly `FIXED_PLUGINS`, none of them
//!   optional (the set sits behind no cargo feature); `tauri`'s merged features include neither
//!   `devtools` nor `protocol-asset`, and no `[features]` entry turns either on through
//!   `tauri`'s key; `tauri-runtime` enables no feature (it has `devtools` and
//!   `macos-private-api`, and no default features, so `default-features` does not matter);
//! - in the desktop crate, the only other family members are `tauri` and `tauri-runtime`: a crate
//!   such as `tauri-runtime-wry` or `wry` has its own `devtools` feature, which the `tauri` checks
//!   never see. Build dependencies (`tauri-build`) and dev-dependencies are not checked;
//! - no other member depends on any family member: cargo unifies features, so a member pulled
//!   into the desktop crate would bring its Tauri features along;
//! - no member depends by path, in any table, on a package that is not a member (a crate under
//!   `[workspace] exclude`): `cargo metadata --no-deps` and clippy never see such a crate.
//!
//! The rules above see workspace members only. A crate under `[workspace] exclude` pulled in by
//! path, or a crates.io or git dependency, never appears in `--no-deps` output, yet it can bring
//! a plugin. So `graph.rs` also reads the desktop crate's own dependency graph, from the
//! `cargo tree -p clauseo-desktop --all-features --locked --offline` run that `checks::graph`
//! builds for `dependencies` too (normal and build edges for every target, all of the desktop
//! crate's features, so a plugin behind an optional dependency of a desktop feature counts, and
//! no feature that only another member or a dev-dependency turns on): a `tauri-plugin-*` outside
//! the fixed set is in that graph only when fixed plugins alone depend on it (as
//! `tauri-plugin-fs` comes in through the dialog plugin). A feature such a crate turns on
//! (`devtools` in tauri, tauri-runtime, tauri-runtime-wry or wry, `protocol-asset` in tauri) is
//! banned in deny.toml's `[[bans.features]]`, which the `deny` step checks.
//!
//! The manifest text is read only for line numbers: the member's `Cargo.toml`, or the root one
//! when the setting lives in `[workspace.dependencies]`, else line 1.
//! The Tauri config and `build.rs` belong to the `tauri-config` check.

mod graph;

use crate::checks::toml_lines::{ANY, Manifest, NORMAL};
use crate::checks::{self, Inspection, Noun, StaticCheck, violation};
use crate::violation::Violation;
use crate::workspace::{CargoDependency, Context, Member, PackageDependencies};

const WORKSPACE_MANIFEST: &str = "Cargo.toml";

/// The only `tauri-plugin-*` packages a desktop build may depend on.
const FIXED_PLUGINS: [&str; 8] = [
    "tauri-plugin-single-instance",
    "tauri-plugin-deep-link",
    "tauri-plugin-updater",
    "tauri-plugin-window-state",
    "tauri-plugin-dialog",
    "tauri-plugin-opener",
    "tauri-plugin-os",
    "tauri-plugin-notification",
];

/// The `tauri` features that must never be enabled.
const FORBIDDEN_FEATURES: [&str; 2] = ["devtools", "protocol-asset"];

const FEATURE_FIX: &str = "remove it: devtools let script in the page open the inspector, and `protocol-asset` serves files from disk to the webview";
const RUNTIME_FIX: &str = "remove it: tauri-runtime is there only to name a type, and its `devtools` and `macos-private-api` features would reach the binary through it";
const PLUGIN_FIX: &str = "remove it: the plugin set is fixed (each change forces a near-cold rebuild and widens what the webview can reach); changing it needs a decision recorded in a task brief";
const OPTIONAL_FIX: &str = "drop `optional = true` and any feature that enables it: the plugin set is fixed and sits behind no cargo feature, so every build ships the same plugins";
const FAMILY_FIX: &str = "depend on `tauri` and the fixed plugins only: other Tauri crates can switch on features such as devtools that the tauri entry's checks never see";
const MEMBER_FIX: &str =
    "only crates/desktop depends on Tauri; move the code there or behind the host interface";
const OUTSIDE_FIX: &str = "add it to the workspace members, or remove the dependency";

/// Fails on every Tauri dependency of a workspace member outside the fixed set or its rules.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "plugins",
    counts: Noun {
        one: "direct dependency",
        many: "direct dependencies",
    },
    place: || {
        format!(
            "of the desktop crate (package `{}`) in `cargo metadata`",
            checks::DESKTOP_PACKAGE
        )
    },
    source: file!(),
    run,
    #[cfg(test)]
    planted: tests::planted::PLANTED,
};

/// `--offline`: the check never downloads. `cargo xtask check` runs `deny` first, which fetches
/// every target's crates; elsewhere a missing crate fails with a fix that says to fetch it. The
/// graph rule runs only when `ctx.load_resolved_graph` allows it; `graph/tests.rs` and
/// `tests/resolved.rs` cover it offline.
fn run(ctx: &Context) -> Inspection {
    check(
        ctx,
        ctx.load_resolved_graph
            .then_some(&["--locked", "--offline"][..]),
    )
}

/// The check, reading the desktop crate's graph with `cargo tree` (`checks::graph::load_tree`,
/// then `graph_flags`), or skipping the graph rule when `graph_flags` is
/// `None` (tests of scratch workspaces that name registry crates, which resolve only with a
/// lockfile and the crates downloaded). Counts the desktop crate's normal and build direct
/// dependencies, the ones its rules read; dev-dependencies are not counted.
fn check(ctx: &Context, graph_flags: Option<&[&str]>) -> Inspection {
    let desktop_member = match checks::desktop_member(ctx) {
        Ok(desktop) => desktop,
        Err(stopped) => return stopped,
    };
    // `desktop_member` stops on a failed load, so the members are here.
    let members = ctx.members.as_deref().unwrap_or_default();
    let manifest = checks::join(&desktop_member.dir, "Cargo.toml");
    let desktop_deps = &desktop_member.dependencies;
    let read = |rel: &str| checks::read_repo_file(&ctx.root, rel).map(Option::unwrap_or_default);
    let (desktop, root) = match (read(&manifest), read(WORKSPACE_MANIFEST)) {
        (Ok(desktop), Ok(root)) => (desktop, root),
        (Err(violation), _) | (_, Err(violation)) => return Inspection::stopped(violation),
    };
    let mut found = violations(
        desktop_deps,
        &Manifests {
            desktop_path: &manifest,
            desktop: &desktop,
            root: &root,
        },
    );
    for member in members {
        let deps = &member.dependencies;
        let path = checks::join(&member.dir, "Cargo.toml");
        let text = if member.name == checks::DESKTOP_PACKAGE {
            desktop.clone()
        } else {
            match read(&path) {
                Ok(text) => {
                    found.extend(member_violations(&member.name, &path, &text, deps));
                    text
                }
                Err(violation) => {
                    found.push(violation);
                    continue;
                }
            }
        };
        found.extend(path_violations(&member.name, &path, &text, deps, members));
    }
    let mut failures = Vec::new();
    if let Some(flags) = graph_flags {
        let desktop = graph::Desktop {
            path: &manifest,
            text: &desktop,
            deps: desktop_deps,
        };
        match checks::graph::load_tree(&ctx.root, checks::DESKTOP_PACKAGE, flags) {
            Ok(tree) => found.extend(graph::violations(&tree, &desktop)),
            Err(failed) => failures.push(failed),
        }
    }
    let read_by_rules = desktop_deps
        .dependencies
        .iter()
        .filter(|dep| dep.kind.as_deref() != Some("dev"))
        .count();
    Inspection {
        failures,
        ..Inspection::new(read_by_rules, found)
    }
}

/// One violation for each path dependency of member `name` whose package is not a workspace
/// member of `members`, such as a crate under
/// `[workspace] exclude`: `cargo metadata --no-deps` and clippy never see it.
fn path_violations(
    name: &str,
    path: &str,
    text: &str,
    deps: &PackageDependencies,
    members: &[Member],
) -> Vec<Violation> {
    let manifest = Manifest::parse(text);
    deps.dependencies
        .iter()
        .filter(|dep| dep.path.is_some() && !members.iter().any(|member| member.name == dep.name))
        .map(|dep| {
            let line = manifest.declaration(dep.key(), ANY).unwrap_or(1);
            violation(
                path,
                line,
                format!(
                    "`{name}` depends on `{dep}` by path, but `{dep}` is not a workspace member, so these checks and clippy never see it",
                    dep = dep.name
                ),
                OUTSIDE_FIX,
            )
        })
        .collect()
}

/// The violations of member `name`, which is not the desktop crate, whose manifest at `path`
/// holds `text`: one for each normal dependency on a Tauri family member, at the line that
/// declares it, else line 1.
pub(crate) fn member_violations(
    name: &str,
    path: &str,
    text: &str,
    deps: &PackageDependencies,
) -> Vec<Violation> {
    let manifest = Manifest::parse(text);
    deps.dependencies
        .iter()
        .filter(|dep| dep.is_normal() && is_tauri(&dep.name))
        .map(|dep| {
            let line = manifest.declaration(dep.key(), NORMAL).unwrap_or(1);
            violation(
                path,
                line,
                format!("`{name}` depends on `{}`{}", dep.name, as_key(dep)),
                MEMBER_FIX,
            )
        })
        .collect()
}

/// True for the Tauri family: `wry`, `tao` and every package whose name starts with `tauri`.
fn is_tauri(package: &str) -> bool {
    package == "wry" || package == "tao" || package.starts_with("tauri")
}

/// ` (as `key`)` when the dependency is renamed, else empty.
fn as_key(dep: &CargoDependency) -> String {
    if dep.key() == dep.name {
        String::new()
    } else {
        format!(" (as `{}`)", dep.key())
    }
}

/// The manifest texts the violations point into.
pub(crate) struct Manifests<'a> {
    /// The repo-relative path of the desktop manifest.
    pub desktop_path: &'a str,
    pub desktop: &'a str,
    /// The root manifest, with `[workspace.dependencies]`.
    pub root: &'a str,
}

/// Every violation in the desktop crate's dependencies `deps`, located in `manifests`.
pub(crate) fn violations(deps: &PackageDependencies, manifests: &Manifests<'_>) -> Vec<Violation> {
    let locate = Locator::new(manifests);
    let normal: Vec<&CargoDependency> = deps
        .dependencies
        .iter()
        .filter(|dep| dep.is_normal())
        .collect();
    let mut found = plugin_violations(&normal, &locate);
    found.extend(family_violations(&normal, &locate));
    found.extend(tauri_feature_violations(&normal, &locate));
    found.extend(feature_table_violations(deps, &normal, &locate));
    found.extend(runtime_feature_violations(&normal, &locate));
    found
}

/// Each family member other than `tauri`, `tauri-runtime` and the `tauri-plugin-*` packages, which
/// `plugin_violations` judges.
fn family_violations(normal: &[&CargoDependency], locate: &Locator<'_>) -> Vec<Violation> {
    normal
        .iter()
        .filter(|dep| {
            let name = dep.name.as_str();
            is_tauri(name)
                && name != "tauri"
                && name != "tauri-runtime"
                && !name.starts_with("tauri-plugin-")
        })
        .map(|dep| {
            locate.dependency(
                dep.key(),
                format!(
                    "`{}`{} is a Tauri crate outside the allowed set",
                    dep.name,
                    as_key(dep)
                ),
                FAMILY_FIX,
            )
        })
        .collect()
}

/// Every feature that a `tauri-runtime` entry enables. It has no default features (checked
/// against tauri-runtime 2.12.2), so only the listed ones count.
fn runtime_feature_violations(normal: &[&CargoDependency], locate: &Locator<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    for dep in normal.iter().filter(|dep| dep.name == "tauri-runtime") {
        for feature in &dep.features {
            let (path, line) = locate.feature(dep.key(), feature);
            found.push(violation(
                path,
                line,
                format!("`tauri-runtime` enables the `{feature}` feature"),
                RUNTIME_FIX,
            ));
        }
    }
    found
}

/// Plugins outside the fixed set, optional fixed plugins, and fixed plugins that are missing.
fn plugin_violations(normal: &[&CargoDependency], locate: &Locator<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    let plugins = normal
        .iter()
        .filter(|dep| dep.name.starts_with("tauri-plugin-"));
    for dep in plugins {
        let as_key = as_key(dep);
        if !FIXED_PLUGINS.contains(&dep.name.as_str()) {
            found.push(locate.dependency(
                dep.key(),
                format!("`{}`{as_key} is not in the fixed plugin set", dep.name),
                PLUGIN_FIX,
            ));
        } else if dep.optional {
            found.push(locate.dependency(
                dep.key(),
                format!("`{}`{as_key} is optional", dep.name),
                OPTIONAL_FIX,
            ));
        }
    }
    for plugin in FIXED_PLUGINS {
        if !normal.iter().any(|dep| dep.name == plugin) {
            found.push(violation(
                locate.desktop_path,
                locate.dependencies_header(),
                format!("`{plugin}` is missing from the fixed plugin set"),
                format!("add it back under [dependencies]: `{plugin}.workspace = true`"),
            ));
        }
    }
    found
}

/// The forbidden features in the merged feature list of each `tauri` entry.
fn tauri_feature_violations(normal: &[&CargoDependency], locate: &Locator<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    for dep in normal.iter().filter(|dep| dep.name == "tauri") {
        for feature in dep
            .features
            .iter()
            .filter(|feature| FORBIDDEN_FEATURES.contains(&feature.as_str()))
        {
            let (path, line) = locate.feature(dep.key(), feature);
            found.push(violation(
                path,
                line,
                format!("`tauri` enables the `{feature}` feature"),
                FEATURE_FIX,
            ));
        }
    }
    found
}

/// Each `[features]` item that turns on a forbidden `tauri` feature: `<key>/<feature>` or
/// `<key>?/<feature>`, where `<key>` is `tauri` or the key a `tauri` entry is renamed to.
fn feature_table_violations(
    deps: &PackageDependencies,
    normal: &[&CargoDependency],
    locate: &Locator<'_>,
) -> Vec<Violation> {
    let mut keys = vec!["tauri"];
    keys.extend(
        normal
            .iter()
            .filter(|dep| dep.name == "tauri")
            .map(|dep| dep.key()),
    );
    let mut found = Vec::new();
    for item in deps.features.values().flatten() {
        let Some((key, feature)) = item.split_once('/') else {
            continue;
        };
        let key = key.strip_suffix('?').unwrap_or(key);
        if keys.contains(&key) && FORBIDDEN_FEATURES.contains(&feature) {
            found.push(violation(
                locate.desktop_path,
                locate.feature_item(item),
                format!("`[features]` enables the `tauri` feature `{feature}` through `{item}`"),
                FEATURE_FIX,
            ));
        }
    }
    found
}

/// Finds the lines that violations point at, in the desktop and root manifests.
struct Locator<'a> {
    desktop_path: &'a str,
    desktop: Manifest<'a>,
    root: Manifest<'a>,
}

impl<'a> Locator<'a> {
    fn new(manifests: &Manifests<'a>) -> Self {
        Locator {
            desktop_path: manifests.desktop_path,
            desktop: Manifest::parse(manifests.desktop),
            root: Manifest::parse(manifests.root),
        }
    }

    /// A violation at the desktop line that declares `key`, else at line 1.
    fn dependency(&self, key: &str, message: String, fix: &str) -> Violation {
        let line = self.desktop.declaration(key, NORMAL).unwrap_or(1);
        violation(self.desktop_path, line, message, fix)
    }

    /// The `[dependencies]` header of the desktop manifest, else line 1.
    fn dependencies_header(&self) -> u32 {
        self.desktop.dependencies_header().unwrap_or(1)
    }

    /// Where the entry for `key` names `feature`: in the desktop entry, else in the root's
    /// `[workspace.dependencies]` entry, else the desktop declaration, else line 1.
    fn feature(&self, key: &str, feature: &str) -> (&'a str, u32) {
        if let Some(line) = self.desktop.feature_in_entry(key, NORMAL, feature) {
            return (self.desktop_path, line);
        }
        if let Some(line) = self.root.workspace_feature(key, feature) {
            return (WORKSPACE_MANIFEST, line);
        }
        let own = self.desktop.declaration(key, NORMAL);
        (self.desktop_path, own.unwrap_or(1))
    }

    /// The `[features]` line of the desktop manifest that holds `"item"`, else line 1.
    fn feature_item(&self, item: &str) -> u32 {
        self.desktop.feature_item(item).unwrap_or(1)
    }
}

#[cfg(test)]
mod tests;
