//! Tests for the `plugins` check: each pure function on a failing and a passing input, and `run`
//! on a scratch tree laid out like the repo.

use super::*;
use crate::scratch::Scratch;
use std::fs;

/// The desktop manifest with the fixed plugin set, as it is committed.
const DESKTOP: &str = r#"[package]
name = "clauseo-desktop"

[build-dependencies]
tauri-build.workspace = true

[dependencies]
tauri.workspace = true
tauri-plugin-single-instance.workspace = true
tauri-plugin-deep-link.workspace = true
tauri-plugin-updater.workspace = true
tauri-plugin-window-state.workspace = true
tauri-plugin-dialog.workspace = true
tauri-plugin-opener.workspace = true
tauri-plugin-os.workspace = true
tauri-plugin-notification.workspace = true
serde_json.workspace = true
"#;

const WORKSPACE: &str = r#"[workspace.dependencies]
tauri = "2.12.4"
tauri-plugin-os = "2.4.0"
"#;

/// Line number of the first line of `text` that contains `needle`.
fn line_of(text: &str, needle: &str) -> u32 {
    let index = text
        .lines()
        .position(|line| line.contains(needle))
        .expect("the needle is in the fixture");
    checks::line_at(index)
}

#[test]
fn the_fixed_plugin_set_passes() {
    assert_eq!(plugin_set_violations(DESKTOP), Vec::new());
}

#[test]
fn an_extra_plugin_fails_at_its_line() {
    let text = DESKTOP.replace(
        "tauri-plugin-notification.workspace = true\n",
        "tauri-plugin-notification.workspace = true\ntauri-plugin-fs = \"2\"\n",
    );
    assert_eq!(
        plugin_set_violations(&text),
        vec![violation(
            DESKTOP_MANIFEST,
            line_of(&text, "tauri-plugin-fs"),
            "`tauri-plugin-fs` is not in the fixed plugin set",
            PLUGIN_FIX,
        )]
    );
}

#[test]
fn a_missing_plugin_fails_at_the_dependencies_header() {
    let text = DESKTOP.replace("tauri-plugin-os.workspace = true\n", "");
    assert_eq!(
        plugin_set_violations(&text),
        vec![violation(
            DESKTOP_MANIFEST,
            line_of(&text, "[dependencies]"),
            "`tauri-plugin-os` is missing from the fixed plugin set",
            "add it back under [dependencies]: `tauri-plugin-os.workspace = true`",
        )]
    );
}

#[test]
fn a_renamed_plugin_counts_under_its_package_name() {
    let renamed_extra = DESKTOP.replace(
        "tauri-plugin-os.workspace = true\n",
        "os = { package = \"tauri-plugin-fs\", version = \"2\" }\n",
    );
    let messages: Vec<String> = plugin_set_violations(&renamed_extra)
        .into_iter()
        .map(|v| v.message)
        .collect();
    assert_eq!(
        messages,
        [
            "`tauri-plugin-fs` is not in the fixed plugin set",
            "`tauri-plugin-os` is missing from the fixed plugin set",
        ]
    );
    let renamed_fixed = DESKTOP.replace(
        "tauri-plugin-os.workspace = true\n",
        "os = { package = \"tauri-plugin-os\", workspace = true }\n",
    );
    assert_eq!(plugin_set_violations(&renamed_fixed), Vec::new());
}

#[test]
fn a_target_table_plugin_counts_and_dev_dependencies_do_not() {
    let target =
        format!("{DESKTOP}\n[target.'cfg(windows)'.dependencies]\ntauri-plugin-fs = \"2\"\n");
    assert_eq!(
        plugin_set_violations(&target),
        vec![violation(
            DESKTOP_MANIFEST,
            line_of(&target, "tauri-plugin-fs"),
            "`tauri-plugin-fs` is not in the fixed plugin set",
            PLUGIN_FIX,
        )]
    );
    let dev = format!("{DESKTOP}\n[dev-dependencies]\ntauri-plugin-fs = \"2\"\n");
    assert_eq!(plugin_set_violations(&dev), Vec::new());
}

#[test]
fn devtools_in_the_desktop_entry_fails_at_its_line() {
    let text = DESKTOP.replace(
        "tauri.workspace = true\n",
        "tauri = { workspace = true, features = [\"devtools\"] }\n",
    );
    assert_eq!(
        tauri_feature_violations(&text, None),
        vec![violation(
            DESKTOP_MANIFEST,
            line_of(&text, "devtools"),
            "`tauri` enables the `devtools` feature",
            FEATURE_FIX,
        )]
    );
    assert_eq!(tauri_feature_violations(DESKTOP, None), Vec::new());
}

#[test]
fn protocol_asset_in_the_workspace_entry_fails_at_its_line() {
    let workspace = WORKSPACE.replace(
        "tauri = \"2.12.4\"",
        "tauri = { version = \"2.12.4\", features = [\n  \"protocol-asset\",\n] }",
    );
    let desktop = DESKTOP.replace("tauri.workspace = true\n", "tauri = { workspace = true }\n");
    assert_eq!(
        tauri_feature_violations(&desktop, Some(&workspace)),
        vec![violation(
            WORKSPACE_MANIFEST,
            line_of(&workspace, "protocol-asset"),
            "`tauri` enables the `protocol-asset` feature",
            FEATURE_FIX,
        )]
    );
    assert_eq!(
        tauri_feature_violations(&desktop, Some(WORKSPACE)),
        Vec::new()
    );
}

#[test]
fn a_workspace_entry_is_read_only_when_the_desktop_entry_inherits_it() {
    let workspace = WORKSPACE.replace(
        "tauri = \"2.12.4\"",
        "tauri = { version = \"2.12.4\", features = [\"devtools\"] }",
    );
    let own = DESKTOP.replace("tauri.workspace = true\n", "tauri = { version = \"2\" }\n");
    assert_eq!(tauri_feature_violations(&own, Some(&workspace)), Vec::new());
}

#[test]
fn a_devtools_reference_in_features_fails_at_its_line() {
    let text = format!("{DESKTOP}\n[features]\ndefault = [\"tauri/devtools\"]\n");
    assert_eq!(
        tauri_feature_violations(&text, None),
        vec![violation(
            DESKTOP_MANIFEST,
            line_of(&text, "tauri/devtools"),
            "`tauri` enables the `devtools` feature",
            FEATURE_FIX,
        )]
    );
    let optional = format!("{DESKTOP}\n[features]\nx = [\"tauri?/protocol-asset\"]\n");
    assert_eq!(tauri_feature_violations(&optional, None).len(), 1);
}

#[test]
fn the_check_is_registered_under_its_name() {
    assert!(
        crate::checks::ALL
            .iter()
            .any(|check| check.name == "plugins")
    );
}

#[test]
fn run_passes_a_clean_tree_and_fails_a_planted_plugin_with_its_path() {
    let dir = Scratch::new("plugins-run");
    let files = [(DESKTOP_MANIFEST, DESKTOP), (WORKSPACE_MANIFEST, WORKSPACE)];
    let write_all = |desktop: &str| {
        for (rel, text) in files {
            let text = if rel == DESKTOP_MANIFEST {
                desktop
            } else {
                text
            };
            let path = dir.join(rel);
            fs::create_dir_all(path.parent().expect("a parent")).expect("create dirs");
            fs::write(path, text).expect("write the file");
        }
    };
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
    };
    write_all(DESKTOP);
    assert_eq!(run(&ctx), Vec::new());
    let planted = DESKTOP.replace(
        "serde_json.workspace = true\n",
        "serde_json.workspace = true\ntauri-plugin-fs = \"2\"\n",
    );
    write_all(&planted);
    assert_eq!(
        run(&ctx),
        vec![violation(
            DESKTOP_MANIFEST,
            line_of(&planted, "tauri-plugin-fs"),
            "`tauri-plugin-fs` is not in the fixed plugin set",
            PLUGIN_FIX,
        )]
    );
}

#[test]
fn run_passes_when_the_desktop_manifest_is_missing() {
    let dir = Scratch::new("plugins-missing");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
    };
    assert_eq!(run(&ctx), Vec::new());
}
