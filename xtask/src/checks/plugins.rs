//! The `plugins` check: the cargo side of the desktop crate, its fixed plugin set and `tauri` features.
//!
//! The dependencies come from `cargo metadata`, which resolves workspace inheritance and renames,
//! so a renamed or inherited entry counts under its real package. Among normal dependencies, with
//! or without a target: the `tauri-plugin-*` packages are exactly `FIXED_PLUGINS`, none of them is
//! optional (the set sits behind no cargo feature), `tauri`'s merged features include neither
//! `devtools` nor `protocol-asset`, and no `[features]` entry turns either on through `tauri`'s key.
//! The manifest text is read only for line numbers: the desktop `Cargo.toml`, or the root one when
//! the setting lives in `[workspace.dependencies]`, else line 1.
//! The Tauri config and `build.rs` belong to the `tauri-config` check.

mod toml_lines;

use crate::checks::{self, StaticCheck, violation};
use crate::violation::Violation;
use crate::workspace::{self, CargoDependency, Context, PackageDependencies};
use toml_lines::{Line, declaration, in_entry, is_dependency_table, is_workspace_table, lines};

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
const PLUGIN_FIX: &str = "remove it: the plugin set is fixed (each change forces a near-cold rebuild and widens what the webview can reach); changing it needs a decision recorded in a task brief";
const OPTIONAL_FIX: &str = "drop `optional = true` and any feature that enables it: the plugin set is fixed and sits behind no cargo feature, so every build ships the same plugins";
const METADATA_FIX: &str = "fix the manifest until `cargo metadata --no-deps` succeeds: this check reads the dependencies from it";

/// Fails on every dependency of the desktop crate outside the fixed plugin set or its rules.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "plugins",
    run,
};

fn run(ctx: &Context) -> Vec<Violation> {
    let dir = match checks::desktop_dir(ctx) {
        Ok(dir) => dir,
        Err(violation) => return vec![violation],
    };
    let manifest = checks::join(dir, "Cargo.toml");
    let desktop = match checks::read_repo_file(&ctx.root, &manifest) {
        Ok(text) => text.unwrap_or_default(),
        Err(violation) => return vec![violation],
    };
    let root = match checks::read_repo_file(&ctx.root, WORKSPACE_MANIFEST) {
        Ok(text) => text.unwrap_or_default(),
        Err(violation) => return vec![violation],
    };
    match workspace::load_package_dependencies(&ctx.root, checks::DESKTOP_PACKAGE) {
        Ok(deps) => violations(
            &deps,
            &Manifests {
                desktop_path: &manifest,
                desktop: &desktop,
                root: &root,
            },
        ),
        Err(error) => vec![violation(
            &manifest,
            1,
            format!(
                "could not read the dependencies of `{}`: {error}",
                checks::DESKTOP_PACKAGE
            ),
            METADATA_FIX,
        )],
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
    found.extend(tauri_feature_violations(&normal, &locate));
    found.extend(feature_table_violations(deps, &normal, &locate));
    found
}

/// Plugins outside the fixed set, optional fixed plugins, and fixed plugins that are missing.
fn plugin_violations(normal: &[&CargoDependency], locate: &Locator<'_>) -> Vec<Violation> {
    let mut found = Vec::new();
    let plugins = normal
        .iter()
        .filter(|dep| dep.name.starts_with("tauri-plugin-"));
    for dep in plugins {
        let as_key = if dep.key() == dep.name {
            String::new()
        } else {
            format!(" (as `{}`)", dep.key())
        };
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
    desktop: Vec<Line<'a>>,
    root: Vec<Line<'a>>,
}

impl<'a> Locator<'a> {
    fn new(manifests: &Manifests<'a>) -> Self {
        Locator {
            desktop_path: manifests.desktop_path,
            desktop: lines(manifests.desktop),
            root: lines(manifests.root),
        }
    }

    /// A violation at the desktop line that declares `key`, else at line 1.
    fn dependency(&self, key: &str, message: String, fix: &str) -> Violation {
        let line = declaration(&self.desktop, key, is_dependency_table).map_or(1, |line| line.no);
        violation(self.desktop_path, line, message, fix)
    }

    /// The `[dependencies]` header of the desktop manifest, else line 1.
    fn dependencies_header(&self) -> u32 {
        self.desktop
            .iter()
            .find(|line| line.text.trim() == "[dependencies]")
            .map_or(1, |line| line.no)
    }

    /// Where the entry for `key` names `feature`: in the desktop entry, else in the root's
    /// `[workspace.dependencies]` entry, else the desktop declaration, else line 1.
    fn feature(&self, key: &str, feature: &str) -> (&'a str, u32) {
        let quoted = format!("\"{feature}\"");
        let own = declaration(&self.desktop, key, is_dependency_table);
        if let Some(line) = own.and_then(|declared| in_entry(&self.desktop, declared, &quoted)) {
            return (self.desktop_path, line.no);
        }
        let inherited = declaration(&self.root, key, is_workspace_table);
        if let Some(line) = inherited.and_then(|declared| in_entry(&self.root, declared, &quoted)) {
            return (WORKSPACE_MANIFEST, line.no);
        }
        (self.desktop_path, own.map_or(1, |line| line.no))
    }

    /// The `[features]` line of the desktop manifest that holds `"item"`, else line 1.
    fn feature_item(&self, item: &str) -> u32 {
        let quoted = format!("\"{item}\"");
        self.desktop
            .iter()
            .find(|line| line.table == "[features]" && line.text.contains(&quoted))
            .map_or(1, |line| line.no)
    }
}

#[cfg(test)]
mod tests;
