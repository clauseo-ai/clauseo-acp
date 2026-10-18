//! The `plugins` check: the fixed plugin set and the security settings of `crates/desktop`.
//!
//! Four things must hold. The `tauri-plugin-*` dependencies are exactly `FIXED_PLUGINS`. `tauri`
//! enables neither `devtools` nor `protocol-asset`, in its entry, its workspace entry or a feature
//! list. `app.withGlobalTauri` in `tauri.conf.json` is `false`. `build.rs` declares an `AppManifest`.
//! The pure functions take file contents; `run` reads the files from the repo.

use crate::checks::{self, StaticCheck};
use crate::violation::Violation;
use crate::workspace::Context;
use std::path::Path;

const DESKTOP_MANIFEST: &str = "crates/desktop/Cargo.toml";
const WORKSPACE_MANIFEST: &str = "Cargo.toml";
const TAURI_CONF: &str = "crates/desktop/tauri.conf.json";
const BUILD_RS: &str = "crates/desktop/build.rs";

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

/// The strings in `[features]` that enable a forbidden `tauri` feature, with that feature's name.
const FEATURE_REFS: [(&str, &str); 4] = [
    ("tauri/devtools", "devtools"),
    ("tauri?/devtools", "devtools"),
    ("tauri/protocol-asset", "protocol-asset"),
    ("tauri?/protocol-asset", "protocol-asset"),
];

const FEATURE_FIX: &str = "remove it: devtools let script in the page open the inspector, and `protocol-asset` serves files from disk to the webview";
const PLUGIN_FIX: &str = "remove it: the plugin set is fixed (each change forces a near-cold rebuild and widens what the webview can reach); changing it needs a decision recorded in a task brief";
const GLOBAL_TAURI_FIX: &str = "set `\"withGlobalTauri\": false` under `app`: the global object hands every script in the page the whole IPC API";
const APP_MANIFEST_FIX: &str = "call `tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[])))`: without a manifest Tauri skips the access check for app commands";

/// Fails on every problem in the desktop manifest, the workspace `tauri` entry, the Tauri config
/// and `build.rs`. Passes when the desktop manifest is missing, so a tree without desktop passes.
pub(crate) const CHECK: StaticCheck = StaticCheck {
    name: "plugins",
    run,
};

fn run(ctx: &Context) -> Vec<Violation> {
    let desktop = match checks::read_repo_file(&ctx.root, DESKTOP_MANIFEST) {
        Ok(Some(text)) => text,
        Ok(None) => return Vec::new(),
        Err(violation) => return vec![violation],
    };
    let mut found = plugin_set_violations(&desktop);
    found.extend(with_file(&ctx.root, WORKSPACE_MANIFEST, |workspace| {
        tauri_feature_violations(&desktop, workspace)
    }));
    found.extend(with_file(&ctx.root, TAURI_CONF, check_global_tauri));
    found.extend(with_file(&ctx.root, BUILD_RS, check_app_manifest));
    found
}

/// Runs `check` on the text of `rel`; `None` means the file is missing. A read error is the violation.
fn with_file(
    root: &Path,
    rel: &str,
    check: impl FnOnce(Option<&str>) -> Vec<Violation>,
) -> Vec<Violation> {
    checks::read_repo_file(root, rel)
        .map_or_else(|violation| vec![violation], |text| check(text.as_deref()))
}

/// The `tauri-plugin-*` dependencies of `text` must be exactly `FIXED_PLUGINS`. A renamed
/// dependency counts under its package name. `[dev-dependencies]` and `[build-dependencies]` do not count.
fn plugin_set_violations(text: &str) -> Vec<Violation> {
    let doc = match parse_manifest(DESKTOP_MANIFEST, text) {
        Ok(doc) => doc,
        Err(violation) => return vec![violation],
    };
    let lines = lines(text);
    let mut found = Vec::new();
    let mut present = Vec::new();
    for (key, entry) in dependencies(&doc) {
        let package = package_name(key, entry);
        if !package.starts_with("tauri-plugin-") {
            continue;
        }
        if FIXED_PLUGINS.contains(&package) {
            present.push(package);
        } else {
            found.push(violation(
                DESKTOP_MANIFEST,
                dependency_line(&lines, key),
                format!("`{package}` is not in the fixed plugin set"),
                PLUGIN_FIX,
            ));
        }
    }
    let header = lines
        .iter()
        .find(|line| line.text.trim() == "[dependencies]")
        .map_or(1, |line| line.no);
    for plugin in FIXED_PLUGINS
        .into_iter()
        .filter(|plugin| !present.contains(plugin))
    {
        found.push(violation(
            DESKTOP_MANIFEST,
            header,
            format!("`{plugin}` is missing from the fixed plugin set"),
            format!("add it back under [dependencies]: `{plugin}.workspace = true`"),
        ));
    }
    found
}

/// The `tauri` features that `desktop` and, where the desktop entry inherits it, `workspace` enable.
/// A syntax error is left to `plugin_set_violations`, which reports it for the same file.
fn tauri_feature_violations(desktop: &str, workspace: Option<&str>) -> Vec<Violation> {
    let Ok(doc) = toml::from_str::<toml::Table>(desktop) else {
        return Vec::new();
    };
    let lines = lines(desktop);
    let mut found = feature_table_violations(&doc, &lines);
    for (key, entry) in dependencies(&doc) {
        if package_name(key, entry) != "tauri" {
            continue;
        }
        let declared = dependency_line(&lines, key);
        found.extend(entry_feature_violations(
            DESKTOP_MANIFEST,
            &lines,
            declared,
            entry,
        ));
        if let Some(workspace) = workspace.filter(|_| inherits_workspace(entry)) {
            found.extend(workspace_tauri_violations(workspace));
        }
    }
    found
}

/// The forbidden features in the root `[workspace.dependencies]` entry for `tauri`, if it has one.
fn workspace_tauri_violations(workspace: &str) -> Vec<Violation> {
    let doc = match parse_manifest(WORKSPACE_MANIFEST, workspace) {
        Ok(doc) => doc,
        Err(violation) => return vec![violation],
    };
    let Some(entry) = doc
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("dependencies"))
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("tauri"))
    else {
        return Vec::new();
    };
    let lines = lines(workspace);
    let declared = declaration_line(&lines, "tauri", |table| table == "[workspace.dependencies]");
    entry_feature_violations(WORKSPACE_MANIFEST, &lines, declared, entry)
}

/// One violation per forbidden feature listed in a `tauri` entry, at the line that names it. The
/// entry starts at `declared`; the feature is named on that line or a later one.
fn entry_feature_violations(
    path: &str,
    lines: &[Line<'_>],
    declared: u32,
    entry: &toml::Value,
) -> Vec<Violation> {
    let features = entry
        .as_table()
        .and_then(|table| table.get("features"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str);
    let mut found = Vec::new();
    for feature in features.filter(|feature| FORBIDDEN_FEATURES.contains(feature)) {
        let quoted = format!("\"{feature}\"");
        let line = lines
            .iter()
            .filter(|line| line.no >= declared)
            .find(|line| line.text.contains(&quoted))
            .map_or(declared, |line| line.no);
        found.push(violation(
            path,
            line,
            format!("`tauri` enables the `{feature}` feature"),
            FEATURE_FIX,
        ));
    }
    found
}

/// One violation per `FEATURE_REFS` string that a value in `[features]` contains, at the first
/// line of that table that names it.
fn feature_table_violations(doc: &toml::Table, lines: &[Line<'_>]) -> Vec<Violation> {
    let Some(features) = doc.get("features").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for item in features
        .values()
        .filter_map(toml::Value::as_array)
        .flatten()
        .filter_map(toml::Value::as_str)
    {
        for (reference, feature) in FEATURE_REFS
            .into_iter()
            .filter(|(reference, _)| item.contains(*reference))
        {
            let line = lines
                .iter()
                .find(|line| line.table == "[features]" && line.text.contains(reference))
                .map_or(1, |line| line.no);
            found.push(violation(
                DESKTOP_MANIFEST,
                line,
                format!("`tauri` enables the `{feature}` feature"),
                FEATURE_FIX,
            ));
        }
    }
    found
}

/// `app.withGlobalTauri` must be present and exactly `false`. A missing file or invalid JSON is a
/// violation at line 1.
fn check_global_tauri(text: Option<&str>) -> Vec<Violation> {
    let Some(text) = text else {
        return vec![violation(
            TAURI_CONF,
            1,
            format!("`{TAURI_CONF}` is missing"),
            GLOBAL_TAURI_FIX,
        )];
    };
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => {
            return vec![violation(
                TAURI_CONF,
                1,
                format!("`{TAURI_CONF}` is not valid JSON: {error}"),
                "fix the JSON syntax so `app.withGlobalTauri` can be read",
            )];
        }
    };
    if value.pointer("/app/withGlobalTauri") == Some(&serde_json::Value::Bool(false)) {
        return Vec::new();
    }
    let line = text
        .lines()
        .position(|line| line.contains("\"withGlobalTauri\""))
        .map_or(1, checks::line_at);
    vec![violation(
        TAURI_CONF,
        line,
        "app.withGlobalTauri must be false",
        GLOBAL_TAURI_FIX,
    )]
}

/// `build.rs` must call `app_manifest(`. A missing file counts as no manifest.
fn check_app_manifest(text: Option<&str>) -> Vec<Violation> {
    if text.is_some_and(|text| text.contains("app_manifest(")) {
        return Vec::new();
    }
    vec![violation(
        BUILD_RS,
        1,
        "build.rs declares no AppManifest",
        APP_MANIFEST_FIX,
    )]
}

fn violation(
    path: &str,
    line: u32,
    message: impl Into<String>,
    fix: impl Into<String>,
) -> Violation {
    Violation {
        path: path.to_owned(),
        line,
        message: message.into(),
        fix: fix.into(),
    }
}

/// Parses a manifest. A syntax error is one violation at line 1.
fn parse_manifest(path: &str, text: &str) -> Result<toml::Table, Violation> {
    toml::from_str(text).map_err(|error| {
        let first = error.message().lines().next().unwrap_or_default().trim();
        violation(
            path,
            1,
            format!("`{path}` is not valid TOML: {first}"),
            "fix the syntax: cargo cannot read the manifest either",
        )
    })
}

/// Every `(key, entry)` in `[dependencies]` and in each `[target.<cfg>.dependencies]`.
fn dependencies(doc: &toml::Table) -> Vec<(&str, &toml::Value)> {
    let mut tables = Vec::new();
    if let Some(table) = doc.get("dependencies").and_then(toml::Value::as_table) {
        tables.push(table);
    }
    if let Some(targets) = doc.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            if let Some(table) = target
                .as_table()
                .and_then(|table| table.get("dependencies"))
                .and_then(toml::Value::as_table)
            {
                tables.push(table);
            }
        }
    }
    tables
        .into_iter()
        .flat_map(|table| table.iter().map(|(key, entry)| (key.as_str(), entry)))
        .collect()
}

/// The package a dependency names: its `package` key when it is renamed, else its own key.
fn package_name<'a>(key: &'a str, entry: &'a toml::Value) -> &'a str {
    entry
        .as_table()
        .and_then(|table| table.get("package"))
        .and_then(toml::Value::as_str)
        .unwrap_or(key)
}

/// True when a dependency entry says `workspace = true`.
fn inherits_workspace(entry: &toml::Value) -> bool {
    entry
        .as_table()
        .and_then(|table| table.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

/// A table header that declares dependencies: `[dependencies]` or `[target.<cfg>.dependencies]`.
fn is_dependency_table(header: &str) -> bool {
    header == "[dependencies]" || header.ends_with(".dependencies]")
}

/// The line that declares dependency `key`: its `key =` line in a table that `in_table` accepts,
/// or its `[dependencies.key]` header. Line 1 when neither is found.
fn declaration_line(lines: &[Line<'_>], key: &str, in_table: impl Fn(&str) -> bool) -> u32 {
    let header = format!("[dependencies.{key}]");
    let sub_table = format!(".dependencies.{key}]");
    lines
        .iter()
        .find(|line| {
            (in_table(line.table) && declares_key(line.text, key))
                || line.table == header
                || line.table.ends_with(&sub_table)
        })
        .map_or(1, |line| line.no)
}

/// `declaration_line` for a dependency of the crate's own manifest.
fn dependency_line(lines: &[Line<'_>], key: &str) -> u32 {
    declaration_line(lines, key, is_dependency_table)
}

/// True when `line` starts an entry for `key`: `key =` or `key.`, with the key bare or double-quoted.
fn declares_key(line: &str, key: &str) -> bool {
    let line = line.trim_start();
    let rest = line.strip_prefix(key).or_else(|| {
        line.strip_prefix('"')
            .and_then(|line| line.strip_prefix(key))
            .and_then(|line| line.strip_prefix('"'))
    });
    rest.is_some_and(|rest| {
        let rest = rest.trim_start();
        rest.starts_with('=') || rest.starts_with('.')
    })
}

/// One line of a TOML file, with the most recent table header above it (`""` before the first).
struct Line<'a> {
    no: u32,
    table: &'a str,
    text: &'a str,
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut table = "";
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('[') {
            table = line.trim();
        }
        lines.push(Line {
            no: checks::line_at(index),
            table,
            text: line,
        });
    }
    lines
}

#[cfg(test)]
mod tests;
