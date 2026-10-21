//! Tests for the `plugins` check. Each case is a tiny scratch workspace (a root `Cargo.toml`,
//! member manifests and empty `src/lib.rs` files) that `run` reads through the real
//! `cargo metadata --no-deps`, so cargo itself resolves the renames and inheritance under test.
//! They skip the graph rules, except the cases in `tests/repos/resolved.rs`, which patch every
//! registry crate to a path crate so the graph resolves offline; `graph/tests.rs` covers the rest.
//! The planted cases are in `tests/planted.rs`; their builders are in `tests/repos.rs`.

pub(super) mod planted;
mod repos;

use super::*;
use crate::checks::planted::scratch as planted_scratch;
use crate::checks::tests::desktop_context;
use crate::scratch::Scratch;
use crate::workspace;

/// The root manifest of a clean workspace.
const ROOT: &str = r#"[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.dependencies]
tauri = "2"
tauri-runtime = "2"
tauri-plugin-single-instance = "2"
tauri-plugin-deep-link = "2"
tauri-plugin-updater = "2"
tauri-plugin-window-state = "2"
tauri-plugin-dialog = "2"
tauri-plugin-opener = "2"
tauri-plugin-os = "2"
tauri-plugin-notification = "2"
"#;

/// The desktop manifest of a clean workspace.
const DESKTOP: &str = r#"[package]
name = "clauseo-desktop"
version = "0.0.0"
edition = "2024"

[dependencies]
tauri.workspace = true
tauri-runtime.workspace = true
tauri-plugin-single-instance.workspace = true
tauri-plugin-deep-link.workspace = true
tauri-plugin-updater.workspace = true
tauri-plugin-window-state.workspace = true
tauri-plugin-dialog.workspace = true
tauri-plugin-opener.workspace = true
tauri-plugin-os.workspace = true
tauri-plugin-notification.workspace = true
"#;

/// The files of a scratch workspace: each path, relative to the workspace, and its text.
type Files = Vec<(String, String)>;

/// One file of a scratch workspace: its `path` and its `text`.
fn file(path: &str, text: &str) -> (String, String) {
    (path.to_owned(), text.to_owned())
}

/// The scratch workspace `planted-plugins-<name>`, with each file of `files` written into it.
/// The directory is removed when the returned `Scratch` drops.
fn scratch_of(name: &str, files: &[(String, String)]) -> Scratch {
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    planted_scratch(&format!("plugins-{name}"), &files)
}

/// The files of a workspace: the root manifest `root`, the desktop manifest `desktop` and one
/// crate per `(dir, manifest)` in `others`, each with an empty `src/lib.rs`.
fn workspace_files(root: &str, desktop: &str, others: &[(&str, &str)]) -> Files {
    let mut files = vec![file("Cargo.toml", root)];
    for (crate_dir, manifest) in [("crates/desktop", desktop)].iter().chain(others) {
        files.push(file(&format!("{crate_dir}/Cargo.toml"), manifest));
        files.push(file(&format!("{crate_dir}/src/lib.rs"), ""));
    }
    files
}

/// The scratch workspace `planted-plugins-<name>` that `workspace_files` describes.
fn write_workspace(name: &str, root: &str, desktop: &str, others: &[(&str, &str)]) -> Scratch {
    scratch_of(name, &workspace_files(root, desktop, others))
}

/// `run` on the scratch workspace `dir`, with the members `cargo metadata` lists.
fn inspect(dir: &Scratch) -> Inspection {
    let workspace = workspace::load(dir).expect("cargo metadata runs");
    let ctx = Context {
        members: Ok(workspace),
        ..desktop_context(dir)
    };
    run(&ctx)
}

/// The fixed set inspects its ten normal dependencies, the count the zero-inspected rule reads.
#[test]
fn the_fixed_set_inspects_its_ten_direct_dependencies() {
    // Its own scratch dir: the planted case `fixed_plugin_set_passes` runs in another thread.
    let dir = write_workspace("fixed_set_count", ROOT, DESKTOP, &[]);
    assert_eq!(inspect(&dir).inspected, 10);
}

/// A build dependency counts, as the graph rule reads it; a dev-dependency does not.
#[test]
fn a_build_dependency_counts_and_a_dev_dependency_does_not() {
    let desktop = format!(
        "{DESKTOP}\n[build-dependencies]\ntauri-build = \"2\"\n\n[dev-dependencies]\nserde = \"1\"\n"
    );
    let dir = write_workspace("counts_build_not_dev", ROOT, &desktop, &[]);
    let found = inspect(&dir);
    assert_eq!((found.inspected, found.violations), (11, Vec::new()));
}
