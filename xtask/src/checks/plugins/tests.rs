//! Tests for the `plugins` check. Each case is a tiny scratch workspace (a root `Cargo.toml`,
//! member manifests and empty `src/lib.rs` files) that `run` reads through the real
//! `cargo metadata --no-deps`, so cargo itself resolves the renames and inheritance under test.
//! They skip the graph rules, except the cases in `tests/resolved.rs`, which patch every registry
//! crate to a path crate so the graph resolves offline; `graph/tests.rs` covers the rest.
//! The planted cases are in `tests/planted.rs`; their builders are the functions below.

pub(super) mod planted;
pub(super) mod resolved;

use super::*;
use crate::checks::tests::desktop_context;
use crate::scratch::Scratch;
use std::fs;
use std::path::Path;

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

/// Writes `text` to `rel` under `dir`, creating its directories.
fn write_file(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().expect("a file has a parent")).expect("create the dirs");
    fs::write(path, text).expect("write the file");
}

/// Writes the scratch workspace `planted-plugins-<name>`: the root manifest `root`, the desktop
/// manifest `desktop` and one crate per `(dir, manifest)` in `others`. The directory is removed
/// when the returned `Scratch` drops.
fn write_workspace(name: &str, root: &str, desktop: &str, others: &[(&str, &str)]) -> Scratch {
    let dir = Scratch::new(&format!("planted-plugins-{name}"));
    write_file(&dir, "Cargo.toml", root);
    for (crate_dir, manifest) in [("crates/desktop", desktop)].iter().chain(others) {
        write_file(&dir, &format!("{crate_dir}/Cargo.toml"), manifest);
        write_file(&dir, &format!("{crate_dir}/src/lib.rs"), "");
    }
    dir
}

/// The clean workspace with `from` replaced by `to` in the desktop manifest.
fn desktop_with(name: &str, from: &str, to: &str) -> Scratch {
    assert!(
        DESKTOP.contains(from),
        "the desktop manifest contains {from}"
    );
    write_workspace(name, ROOT, &DESKTOP.replacen(from, to, 1), &[])
}

/// A member crate named `name` whose manifest is the package header and then `body`.
fn member(name: &str, body: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n{body}")
}

/// `run` on the scratch workspace `dir`, with the members `cargo metadata` lists.
fn inspect(dir: &Scratch) -> Inspection {
    let ctx = Context {
        members: workspace::load_members(dir).expect("cargo metadata runs"),
        ..desktop_context(dir)
    };
    run(&ctx)
}

pub(super) fn fixed_plugin_set_passes() -> Inspection {
    inspect(&write_workspace(
        "fixed_plugin_set_passes",
        ROOT,
        DESKTOP,
        &[],
    ))
}

pub(super) fn tauri_build_as_a_build_dependency_passes() -> Inspection {
    let desktop = format!("{DESKTOP}\n[build-dependencies]\ntauri-build = \"2\"\n");
    inspect(&write_workspace("tauri_build_passes", ROOT, &desktop, &[]))
}

pub(super) fn dev_dependency_plugin_passes() -> Inspection {
    let desktop = format!("{DESKTOP}\n[dev-dependencies]\ntauri-plugin-shell = \"2\"\n");
    inspect(&write_workspace(
        "dev_dependency_plugin",
        ROOT,
        &desktop,
        &[],
    ))
}

pub(super) fn plugin_outside_fixed_set() -> Inspection {
    let desktop = format!("{DESKTOP}tauri-plugin-shell = \"2\"\n");
    inspect(&write_workspace(
        "plugin_outside_fixed_set",
        ROOT,
        &desktop,
        &[],
    ))
}

pub(super) fn renamed_plugin_outside_fixed_set() -> Inspection {
    let root = format!("{ROOT}files = {{ package = \"tauri-plugin-fs\", version = \"2\" }}\n");
    let desktop = format!("{DESKTOP}files.workspace = true\n");
    inspect(&write_workspace("renamed_plugin", &root, &desktop, &[]))
}

pub(super) fn target_plugin_outside_fixed_set() -> Inspection {
    let desktop =
        format!("{DESKTOP}\n[target.'cfg(windows)'.dependencies]\ntauri-plugin-fs = \"2\"\n");
    inspect(&write_workspace("target_plugin", ROOT, &desktop, &[]))
}

pub(super) fn optional_fixed_plugin() -> Inspection {
    let desktop = DESKTOP.replacen(
        "tauri-plugin-dialog.workspace = true\n",
        "tauri-plugin-dialog = { workspace = true, optional = true }\n",
        1,
    );
    let desktop = format!("{desktop}\n[features]\ndefault = [\"tauri-plugin-dialog\"]\n");
    inspect(&write_workspace(
        "optional_fixed_plugin",
        ROOT,
        &desktop,
        &[],
    ))
}

pub(super) fn fixed_plugin_missing() -> Inspection {
    inspect(&desktop_with(
        "fixed_plugin_missing",
        "tauri-plugin-os.workspace = true\n",
        "",
    ))
}

pub(super) fn wry_in_desktop() -> Inspection {
    let desktop = format!(
        "{DESKTOP}\n[target.'cfg(unix)'.dependencies]\nwry = {{ version = \"0.53\", features = [\"devtools\"] }}\n"
    );
    inspect(&write_workspace("wry_in_desktop", ROOT, &desktop, &[]))
}

pub(super) fn tauri_runtime_wry_in_desktop() -> Inspection {
    let desktop = format!(
        "{DESKTOP}tauri-runtime-wry = {{ version = \"2.12.2\", features = [\"devtools\"] }}\n"
    );
    inspect(&write_workspace("tauri_runtime_wry", ROOT, &desktop, &[]))
}

pub(super) fn tauri_runtime_devtools_in_desktop() -> Inspection {
    inspect(&desktop_with(
        "tauri_runtime_devtools",
        "tauri-runtime.workspace = true\n",
        "tauri-runtime = { workspace = true, default-features = true, features = [\"devtools\"] }\n",
    ))
}

pub(super) fn tauri_runtime_feature_in_workspace() -> Inspection {
    let root = ROOT.replace(
        "tauri-runtime = \"2\"\n",
        "tauri-runtime = { version = \"2\", features = [\"macos-private-api\"] }\n",
    );
    inspect(&write_workspace("tauri_runtime_root", &root, DESKTOP, &[]))
}

pub(super) fn devtools_in_tauri_entry() -> Inspection {
    inspect(&desktop_with(
        "devtools_in_tauri_entry",
        "tauri.workspace = true\n",
        "tauri = { workspace = true, features = [\n  \"devtools\",\n] }\n",
    ))
}

pub(super) fn protocol_asset_in_tauri_entry() -> Inspection {
    inspect(&desktop_with(
        "protocol_asset_in_tauri_entry",
        "tauri.workspace = true\n",
        "tauri = { workspace = true, features = [\"protocol-asset\"] }\n",
    ))
}

pub(super) fn devtools_in_renamed_tauri_entry() -> Inspection {
    let root = ROOT.replace(
        "tauri = \"2\"\n",
        "t = { package = \"tauri\", version = \"2\", features = [\"devtools\"] }\n",
    );
    let desktop = DESKTOP.replace("tauri.workspace = true\n", "t.workspace = true\n");
    inspect(&write_workspace(
        "renamed_tauri_devtools",
        &root,
        &desktop,
        &[],
    ))
}

pub(super) fn protocol_asset_in_renamed_tauri_entry() -> Inspection {
    let root = ROOT.replace(
        "tauri = \"2\"\n",
        "t = { package = \"tauri\", version = \"2\", features = [\"protocol-asset\"] }\n",
    );
    let desktop = DESKTOP.replace("tauri.workspace = true\n", "t.workspace = true\n");
    inspect(&write_workspace(
        "renamed_tauri_protocol_asset",
        &root,
        &desktop,
        &[],
    ))
}

pub(super) fn feature_table_enables_devtools_through_tauri() -> Inspection {
    let desktop = format!("{DESKTOP}\n[features]\ninspect = [\"tauri/devtools\"]\n");
    inspect(&write_workspace("feature_table_tauri", ROOT, &desktop, &[]))
}

pub(super) fn feature_table_enables_through_a_rename() -> Inspection {
    let root = ROOT.replace(
        "tauri = \"2\"\n",
        "t = { package = \"tauri\", version = \"2\" }\n",
    );
    let desktop = format!(
        "{}\n[features]\nasset = [\"t/protocol-asset\"]\n",
        DESKTOP.replace("tauri.workspace = true\n", "t.workspace = true\n")
    );
    inspect(&write_workspace(
        "feature_table_rename",
        &root,
        &desktop,
        &[],
    ))
}

pub(super) fn feature_table_enables_through_an_optional_tauri() -> Inspection {
    let desktop = DESKTOP.replacen(
        "tauri.workspace = true\n",
        "tauri = { workspace = true, optional = true }\n",
        1,
    );
    let desktop = format!("{desktop}\n[features]\ninspect = [\"tauri?/devtools\"]\n");
    inspect(&write_workspace(
        "feature_table_optional",
        ROOT,
        &desktop,
        &[],
    ))
}

pub(super) fn other_member_depends_on_tauri() -> Inspection {
    let core = member("clauseo-core", "[dependencies]\ntauri = \"2\"\n");
    inspect(&write_workspace(
        "member_tauri",
        ROOT,
        DESKTOP,
        &[("crates/core", core.as_str())],
    ))
}

pub(super) fn other_member_depends_on_wry() -> Inspection {
    let core = member("clauseo-core", "[dependencies]\nwry = \"0.53\"\n");
    inspect(&write_workspace(
        "member_wry",
        ROOT,
        DESKTOP,
        &[("crates/core", core.as_str())],
    ))
}

pub(super) fn other_member_renamed_tauri_runtime_in_target() -> Inspection {
    let core = member(
        "clauseo-core",
        "[target.'cfg(unix)'.dependencies]\nrt = { package = \"tauri-runtime\", version = \"2\" }\n",
    );
    inspect(&write_workspace(
        "member_renamed_runtime",
        ROOT,
        DESKTOP,
        &[("crates/core", core.as_str())],
    ))
}

pub(super) fn other_member_depends_on_tauri_through_a_path_dependency() -> Inspection {
    let root = ROOT.replace(
        "members = [\"crates/*\"]\n",
        "members = [\"crates/*\"]\nexclude = [\"vendor\"]\n",
    );
    let core = member(
        "clauseo-core",
        "[dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
    );
    let dir = write_workspace(
        "member_tauri_path",
        &root,
        DESKTOP,
        &[("crates/core", core.as_str())],
    );
    write_file(
        &dir,
        "vendor/tauri/Cargo.toml",
        "[package]\nname = \"tauri\"\nversion = \"2.0.0\"\nedition = \"2024\"\n",
    );
    write_file(&dir, "vendor/tauri/src/lib.rs", "");
    inspect(&dir)
}

pub(super) fn path_dependency_on_excluded_crate() -> Inspection {
    let root = ROOT.replace(
        "members = [\"crates/*\"]\n",
        "members = [\"crates/*\"]\nexclude = [\"evil\"]\n",
    );
    let desktop = format!("{DESKTOP}evil = {{ path = \"../../evil\" }}\n");
    let dir = write_workspace("path_dependency_excluded", &root, &desktop, &[]);
    write_file(
        &dir,
        "evil/Cargo.toml",
        "[package]\nname = \"evil\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
         [dependencies]\ntauri-runtime-wry = { version = \"2\", features = [\"devtools\"] }\n",
    );
    write_file(&dir, "evil/src/lib.rs", "");
    inspect(&dir)
}

pub(super) fn metadata_lists_no_desktop_package() -> Inspection {
    // The desktop crate is in the context but not in the workspace, so `cargo metadata` lists no
    // package with its name. A failing `cargo metadata` is covered by the test below.
    let dir = Scratch::new("planted-plugins-metadata_lists_no_desktop_package");
    write_file(
        &dir,
        "Cargo.toml",
        "[workspace]\nresolver = \"3\"\nmembers = [\"crates/other\"]\n",
    );
    write_file(
        &dir,
        "crates/other/Cargo.toml",
        &member("clauseo-other", ""),
    );
    write_file(&dir, "crates/other/src/lib.rs", "");
    run(&desktop_context(&dir))
}

pub(super) fn desktop_crate_missing() -> Inspection {
    let dir = Scratch::new("planted-plugins-desktop_crate_missing");
    write_file(&dir, "Cargo.toml", ROOT);
    run(&Context {
        members: Vec::new(),
        ..desktop_context(&dir)
    })
}

/// The fixed set inspects its ten normal dependencies, the count the zero-inspected rule reads.
#[test]
fn the_fixed_set_inspects_its_ten_direct_dependencies() {
    assert_eq!(fixed_plugin_set_passes().inspected, 10);
}

/// `cargo metadata` fails when the root manifest's `tauri` requirement does not parse. Its
/// message ends with cargo's stderr, which names the scratch directory (with its pid), so no
/// planted case can hold the text exactly: this test checks the parts that do not vary.
#[test]
fn a_cargo_metadata_failure_stops_the_check_at_the_desktop_manifest() {
    let root = ROOT.replace("tauri = \"2\"\n", "tauri = \"not a req\"\n");
    let dir = write_workspace("metadata_fails", &root, DESKTOP, &[]);
    let found = run(&desktop_context(&dir));
    assert_eq!(found.inspected, 0);
    let [only] = found.violations.as_slice() else {
        panic!("one violation, found {:?}", found.violations)
    };
    assert_eq!(
        (only.path.as_str(), only.line, only.fix.as_str()),
        ("crates/desktop/Cargo.toml", 1, METADATA_FIX)
    );
    assert!(
        only.message
            .starts_with("could not read the workspace's dependencies: `cargo metadata` failed: "),
        "{}",
        only.message
    );
}
