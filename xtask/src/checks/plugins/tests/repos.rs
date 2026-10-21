//! The builders of the plugins planted cases: a cargo workspace in a scratch folder, its clean
//! manifests (`ROOT`, `DESKTOP`) with one thing changed. The cases that turn the graph on are by
//! topic in `repos/resolved.rs`.

mod resolved;

pub(super) use resolved::*;

use super::*;
use crate::checks::planted::Repo;
use crate::scratch::Scratch;

/// A member crate named `name` whose manifest is the package header and then `body`.
fn member(name: &str, body: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n{body}")
}

/// The clean workspace with `from` replaced by `to` in the desktop manifest.
fn desktop_with(name: &str, from: &str, to: &str) -> Scratch {
    assert!(
        DESKTOP.contains(from),
        "the desktop manifest contains {from}"
    );
    write_workspace(name, ROOT, &DESKTOP.replacen(from, to, 1), &[])
}

pub(super) fn fixed_plugin_set_passes() -> Repo {
    Repo::cargo(write_workspace(
        "fixed_plugin_set_passes",
        ROOT,
        DESKTOP,
        &[],
    ))
}

pub(super) fn tauri_build_as_a_build_dependency_passes() -> Repo {
    let desktop = format!("{DESKTOP}\n[build-dependencies]\ntauri-build = \"2\"\n");
    Repo::cargo(write_workspace("tauri_build_passes", ROOT, &desktop, &[]))
}

pub(super) fn dev_dependency_plugin_passes() -> Repo {
    let desktop = format!("{DESKTOP}\n[dev-dependencies]\ntauri-plugin-shell = \"2\"\n");
    Repo::cargo(write_workspace(
        "dev_dependency_plugin",
        ROOT,
        &desktop,
        &[],
    ))
}

pub(super) fn plugin_outside_fixed_set() -> Repo {
    let desktop = format!("{DESKTOP}tauri-plugin-shell = \"2\"\n");
    Repo::cargo(write_workspace(
        "plugin_outside_fixed_set",
        ROOT,
        &desktop,
        &[],
    ))
}

pub(super) fn renamed_plugin_outside_fixed_set() -> Repo {
    let root = format!("{ROOT}files = {{ package = \"tauri-plugin-fs\", version = \"2\" }}\n");
    let desktop = format!("{DESKTOP}files.workspace = true\n");
    Repo::cargo(write_workspace("renamed_plugin", &root, &desktop, &[]))
}

pub(super) fn target_plugin_outside_fixed_set() -> Repo {
    let desktop =
        format!("{DESKTOP}\n[target.'cfg(windows)'.dependencies]\ntauri-plugin-fs = \"2\"\n");
    Repo::cargo(write_workspace("target_plugin", ROOT, &desktop, &[]))
}

pub(super) fn optional_fixed_plugin() -> Repo {
    let desktop = DESKTOP.replacen(
        "tauri-plugin-dialog.workspace = true\n",
        "tauri-plugin-dialog = { workspace = true, optional = true }\n",
        1,
    );
    let desktop = format!("{desktop}\n[features]\ndefault = [\"tauri-plugin-dialog\"]\n");
    Repo::cargo(write_workspace(
        "optional_fixed_plugin",
        ROOT,
        &desktop,
        &[],
    ))
}

pub(super) fn fixed_plugin_missing() -> Repo {
    Repo::cargo(desktop_with(
        "fixed_plugin_missing",
        "tauri-plugin-os.workspace = true\n",
        "",
    ))
}

pub(super) fn wry_in_desktop() -> Repo {
    let desktop = format!(
        "{DESKTOP}\n[target.'cfg(unix)'.dependencies]\nwry = {{ version = \"0.53\", features = [\"devtools\"] }}\n"
    );
    Repo::cargo(write_workspace("wry_in_desktop", ROOT, &desktop, &[]))
}

pub(super) fn tauri_runtime_wry_in_desktop() -> Repo {
    let desktop = format!(
        "{DESKTOP}tauri-runtime-wry = {{ version = \"2.12.2\", features = [\"devtools\"] }}\n"
    );
    Repo::cargo(write_workspace("tauri_runtime_wry", ROOT, &desktop, &[]))
}

pub(super) fn tauri_runtime_devtools_in_desktop() -> Repo {
    Repo::cargo(desktop_with(
        "tauri_runtime_devtools",
        "tauri-runtime.workspace = true\n",
        "tauri-runtime = { workspace = true, default-features = true, features = [\"devtools\"] }\n",
    ))
}

pub(super) fn tauri_runtime_feature_in_workspace() -> Repo {
    let root = ROOT.replace(
        "tauri-runtime = \"2\"\n",
        "tauri-runtime = { version = \"2\", features = [\"macos-private-api\"] }\n",
    );
    Repo::cargo(write_workspace("tauri_runtime_root", &root, DESKTOP, &[]))
}

pub(super) fn devtools_in_tauri_entry() -> Repo {
    Repo::cargo(desktop_with(
        "devtools_in_tauri_entry",
        "tauri.workspace = true\n",
        "tauri = { workspace = true, features = [\n  \"devtools\",\n] }\n",
    ))
}

pub(super) fn protocol_asset_in_tauri_entry() -> Repo {
    Repo::cargo(desktop_with(
        "protocol_asset_in_tauri_entry",
        "tauri.workspace = true\n",
        "tauri = { workspace = true, features = [\"protocol-asset\"] }\n",
    ))
}

pub(super) fn devtools_in_renamed_tauri_entry() -> Repo {
    let root = ROOT.replace(
        "tauri = \"2\"\n",
        "t = { package = \"tauri\", version = \"2\", features = [\"devtools\"] }\n",
    );
    let desktop = DESKTOP.replace("tauri.workspace = true\n", "t.workspace = true\n");
    Repo::cargo(write_workspace(
        "renamed_tauri_devtools",
        &root,
        &desktop,
        &[],
    ))
}

pub(super) fn protocol_asset_in_renamed_tauri_entry() -> Repo {
    let root = ROOT.replace(
        "tauri = \"2\"\n",
        "t = { package = \"tauri\", version = \"2\", features = [\"protocol-asset\"] }\n",
    );
    let desktop = DESKTOP.replace("tauri.workspace = true\n", "t.workspace = true\n");
    Repo::cargo(write_workspace(
        "renamed_tauri_protocol_asset",
        &root,
        &desktop,
        &[],
    ))
}

pub(super) fn feature_table_enables_devtools_through_tauri() -> Repo {
    let desktop = format!("{DESKTOP}\n[features]\ninspect = [\"tauri/devtools\"]\n");
    Repo::cargo(write_workspace("feature_table_tauri", ROOT, &desktop, &[]))
}

pub(super) fn feature_table_enables_through_a_rename() -> Repo {
    let root = ROOT.replace(
        "tauri = \"2\"\n",
        "t = { package = \"tauri\", version = \"2\" }\n",
    );
    let desktop = format!(
        "{}\n[features]\nasset = [\"t/protocol-asset\"]\n",
        DESKTOP.replace("tauri.workspace = true\n", "t.workspace = true\n")
    );
    Repo::cargo(write_workspace(
        "feature_table_rename",
        &root,
        &desktop,
        &[],
    ))
}

pub(super) fn feature_table_enables_through_an_optional_tauri() -> Repo {
    let desktop = DESKTOP.replacen(
        "tauri.workspace = true\n",
        "tauri = { workspace = true, optional = true }\n",
        1,
    );
    let desktop = format!("{desktop}\n[features]\ninspect = [\"tauri?/devtools\"]\n");
    Repo::cargo(write_workspace(
        "feature_table_optional",
        ROOT,
        &desktop,
        &[],
    ))
}

pub(super) fn other_member_depends_on_tauri() -> Repo {
    let core = member("clauseo-core", "[dependencies]\ntauri = \"2\"\n");
    Repo::cargo(write_workspace(
        "member_tauri",
        ROOT,
        DESKTOP,
        &[("crates/core", core.as_str())],
    ))
}

pub(super) fn other_member_depends_on_wry() -> Repo {
    let core = member("clauseo-core", "[dependencies]\nwry = \"0.53\"\n");
    Repo::cargo(write_workspace(
        "member_wry",
        ROOT,
        DESKTOP,
        &[("crates/core", core.as_str())],
    ))
}

pub(super) fn other_member_renamed_tauri_runtime_in_target() -> Repo {
    let core = member(
        "clauseo-core",
        "[target.'cfg(unix)'.dependencies]\nrt = { package = \"tauri-runtime\", version = \"2\" }\n",
    );
    Repo::cargo(write_workspace(
        "member_renamed_runtime",
        ROOT,
        DESKTOP,
        &[("crates/core", core.as_str())],
    ))
}

pub(super) fn other_member_depends_on_tauri_through_a_path_dependency() -> Repo {
    let root = ROOT.replace(
        "members = [\"crates/*\"]\n",
        "members = [\"crates/*\"]\nexclude = [\"vendor\"]\n",
    );
    let core = member(
        "clauseo-core",
        "[dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
    );
    let mut files = workspace_files(&root, DESKTOP, &[("crates/core", core.as_str())]);
    files.push(file(
        "vendor/tauri/Cargo.toml",
        "[package]\nname = \"tauri\"\nversion = \"2.0.0\"\nedition = \"2024\"\n",
    ));
    files.push(file("vendor/tauri/src/lib.rs", ""));
    Repo::cargo(scratch_of("member_tauri_path", &files))
}

pub(super) fn path_dependency_on_excluded_crate() -> Repo {
    let root = ROOT.replace(
        "members = [\"crates/*\"]\n",
        "members = [\"crates/*\"]\nexclude = [\"evil\"]\n",
    );
    let desktop = format!("{DESKTOP}evil = {{ path = \"../../evil\" }}\n");
    let mut files = workspace_files(&root, &desktop, &[]);
    files.push(file(
        "evil/Cargo.toml",
        "[package]\nname = \"evil\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
         [dependencies]\ntauri-runtime-wry = { version = \"2\", features = [\"devtools\"] }\n",
    ));
    files.push(file("evil/src/lib.rs", ""));
    Repo::cargo(scratch_of("path_dependency_excluded", &files))
}

pub(super) fn desktop_crate_missing() -> Repo {
    Repo::files("plugins-desktop-crate-missing", &[("Cargo.toml", ROOT)])
}
