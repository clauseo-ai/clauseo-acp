//! Tests for the graph rules of the `plugins` check. Each case is a scratch workspace of path
//! crates named like the registry crates they stand for (`wry`, `tauri`, `tauri-plugin-*`), so
//! the real `cargo metadata --offline` resolves the graph without a lockfile or the network.

use super::*;
use crate::scratch::Scratch;
use crate::workspace;
use std::fs;

const DESKTOP_PATH: &str = "crates/desktop/Cargo.toml";

/// A scratch workspace with the desktop manifest `desktop` and one path crate per
/// `(name, manifest body)` in `crates`, each under `vendor/<name>`.
fn workspace(name: &str, desktop: &str, crates: &[(&str, &str)]) -> Scratch {
    let dir = Scratch::new(&format!("plugins-graph-{name}"));
    let root = "[workspace]\nresolver = \"3\"\nmembers = [\"crates/desktop\"]\n";
    fs::write(dir.join("Cargo.toml"), root).expect("write the root manifest");
    let mut all = vec![("crates/desktop".to_owned(), desktop.to_owned())];
    for (crate_name, body) in crates {
        let manifest = format!(
            "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{body}"
        );
        all.push((format!("vendor/{crate_name}"), manifest));
    }
    for (crate_dir, manifest) in all {
        fs::create_dir_all(dir.join(&crate_dir).join("src")).expect("create the crate dirs");
        fs::write(dir.join(&crate_dir).join("src/lib.rs"), "").expect("write lib.rs");
        fs::write(dir.join(&crate_dir).join("Cargo.toml"), manifest).expect("write a manifest");
    }
    dir
}

/// The graph violations of the workspace in `dir`, whose desktop manifest is `desktop`.
fn violations_in(dir: &Scratch, desktop: &str) -> Vec<Violation> {
    let graph = workspace::load_resolved_graph(dir, &["--offline"]).expect("cargo metadata runs");
    let packages = workspace::load_workspace_dependencies(dir).expect("cargo metadata runs");
    violations(
        &graph,
        &Desktop {
            path: DESKTOP_PATH,
            text: desktop,
            deps: &packages[checks::DESKTOP_PACKAGE],
        },
    )
}

/// Line number of the first line of `text` that contains `needle`.
fn line_of(text: &str, needle: &str) -> u32 {
    let index = text
        .lines()
        .position(|line| line.contains(needle))
        .expect("the needle is in the manifest");
    checks::line_at(index)
}

/// The desktop manifest header, followed by `deps`.
fn desktop(deps: &str) -> String {
    format!(
        "[package]\nname = \"clauseo-desktop\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n{deps}"
    )
}

#[test]
fn a_plugin_outside_the_set_fails_unless_only_fixed_plugins_pull_it_in() {
    let desktop = desktop(
        "[dependencies]\ntauri-plugin-dialog = { path = \"../../vendor/tauri-plugin-dialog\" }\ntauri-plugin = { path = \"../../vendor/tauri-plugin\" }\nmid = { path = \"../../vendor/mid\" }\n\n[build-dependencies]\ntauri-plugin-yy = { path = \"../../vendor/tauri-plugin-yy\" }\n\n[dev-dependencies]\ntauri-plugin-dev = { path = \"../../vendor/tauri-plugin-dev\" }\n",
    );
    let plugin = "[dependencies]\ntauri-plugin = { path = \"../tauri-plugin\" }\n";
    let ws = workspace(
        "tree",
        &desktop,
        &[
            (
                "tauri-plugin-dialog",
                "[dependencies]\ntauri-plugin-fs = { path = \"../tauri-plugin-fs\" }\n",
            ),
            ("tauri-plugin-fs", plugin),
            ("tauri-plugin", ""),
            (
                "mid",
                "[dependencies]\ntauri-plugin-zz = { path = \"../tauri-plugin-zz\" }\n",
            ),
            ("tauri-plugin-zz", plugin),
            ("tauri-plugin-yy", ""),
            ("tauri-plugin-dev", ""),
        ],
    );
    let at = |needle: &str, message: &str| {
        violation(DESKTOP_PATH, line_of(&desktop, needle), message, TREE_FIX)
    };
    assert_eq!(
        violations_in(&ws, &desktop),
        vec![
            at(
                "mid = ",
                "`tauri-plugin-zz` 0.1.0 is in the desktop crate's tree through `mid`, which is not a fixed plugin",
            ),
            at(
                "tauri-plugin-yy = ",
                "`tauri-plugin-yy` 0.1.0 is in the desktop crate's tree through `clauseo-desktop`, which is not a fixed plugin",
            ),
        ]
    );
}

#[test]
fn a_clean_graph_passes() {
    let desktop = desktop("[dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n");
    let ws = workspace("clean", &desktop, &[("tauri", "")]);
    assert_eq!(violations_in(&ws, &desktop), Vec::new());
}
