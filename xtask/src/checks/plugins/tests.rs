//! Tests for the `plugins` check. The pure rules run on `cargo metadata` JSON copied from small
//! scratch workspaces (`fixtures/<case>.json`, trimmed to the fields xtask reads, with the root
//! path replaced by `/ws`), next to the two manifests that produced it. `run` calls the real
//! `cargo metadata` on a scratch workspace.

use super::*;
use crate::checks::tests::desktop_context;
use crate::scratch::Scratch;
use crate::workspace::parse_package_dependencies;
use std::fs;

const DESKTOP_PATH: &str = "crates/desktop/Cargo.toml";

/// One fixture: the parsed metadata, the desktop manifest and the root manifest.
struct Case {
    deps: PackageDependencies,
    desktop: String,
    root: String,
}

macro_rules! case {
    ($name:literal) => {
        Case {
            deps: parse_package_dependencies(
                include_str!(concat!("fixtures/", $name, ".json")),
                "clauseo-desktop",
            )
            .expect("the fixture parses"),
            desktop: include_str!(concat!("fixtures/", $name, ".desktop.toml")).to_owned(),
            root: include_str!(concat!("fixtures/", $name, ".root.toml")).to_owned(),
        }
    };
}

impl Case {
    fn violations(&self) -> Vec<Violation> {
        violations(
            &self.deps,
            &Manifests {
                desktop_path: DESKTOP_PATH,
                desktop: &self.desktop,
                root: &self.root,
            },
        )
    }

    /// The dependency named `name` (the first, if there are several).
    fn dep(&mut self, name: &str) -> &mut CargoDependency {
        self.deps
            .dependencies
            .iter_mut()
            .find(|dep| dep.name == name)
            .expect("the fixture has the dependency")
    }
}

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
    assert_eq!(case!("clean").violations(), Vec::new());
}

#[test]
fn an_optional_fixed_plugin_behind_a_feature_fails() {
    let case = case!("optional");
    assert_eq!(
        case.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&case.desktop, "optional = true"),
            "`tauri-plugin-dialog` is optional",
            OPTIONAL_FIX,
        )]
    );
}

#[test]
fn a_plugin_renamed_in_workspace_dependencies_counts_under_its_package() {
    let case = case!("renamed_plugin");
    assert_eq!(
        case.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&case.desktop, "files.workspace = true"),
            "`tauri-plugin-fs` (as `files`) is not in the fixed plugin set",
            PLUGIN_FIX,
        )]
    );
}

#[test]
fn a_renamed_tauri_with_devtools_and_a_feature_through_its_rename_fail() {
    let case = case!("renamed_tauri");
    assert_eq!(
        case.violations(),
        vec![
            violation(
                WORKSPACE_MANIFEST,
                line_of(&case.root, "package = \"tauri\""),
                "`tauri` enables the `devtools` feature",
                FEATURE_FIX,
            ),
            violation(
                DESKTOP_PATH,
                line_of(&case.desktop, "t/protocol-asset"),
                "`[features]` enables the `tauri` feature `protocol-asset` through `t/protocol-asset`",
                FEATURE_FIX,
            ),
        ]
    );
}

#[test]
fn a_target_plugin_counts_and_a_dev_plugin_does_not() {
    let case = case!("target");
    assert_eq!(
        case.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&case.desktop, "tauri-plugin-fs"),
            "`tauri-plugin-fs` is not in the fixed plugin set",
            PLUGIN_FIX,
        )]
    );
}

#[test]
fn a_missing_plugin_fails_at_the_dependencies_header() {
    let mut case = case!("clean");
    case.deps
        .dependencies
        .retain(|dep| dep.name != "tauri-plugin-os");
    case.desktop = case
        .desktop
        .replace("tauri-plugin-os.workspace = true\n", "");
    assert_eq!(
        case.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&case.desktop, "[dependencies]"),
            "`tauri-plugin-os` is missing from the fixed plugin set",
            "add it back under [dependencies]: `tauri-plugin-os.workspace = true`",
        )]
    );
}

#[test]
fn devtools_in_the_desktop_entry_fails_at_its_line() {
    let mut case = case!("clean");
    case.dep("tauri").features = vec!["devtools".to_owned()];
    case.desktop = case.desktop.replace(
        "tauri.workspace = true\n",
        "tauri = { workspace = true, features = [\n  \"devtools\",\n] }\n",
    );
    assert_eq!(
        case.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&case.desktop, "\"devtools\""),
            "`tauri` enables the `devtools` feature",
            FEATURE_FIX,
        )]
    );
}

#[test]
fn an_optional_tauri_feature_reference_fails() {
    let mut case = case!("clean");
    case.deps.features.insert(
        "inspect".to_owned(),
        vec!["tauri?/devtools".to_owned(), "tauri-x/devtools".to_owned()],
    );
    case.desktop
        .push_str("\n[features]\ninspect = [\"tauri?/devtools\", \"tauri-x/devtools\"]\n");
    assert_eq!(
        case.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&case.desktop, "inspect ="),
            "`[features]` enables the `tauri` feature `devtools` through `tauri?/devtools`",
            FEATURE_FIX,
        )]
    );
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
fn run_reads_real_cargo_metadata_and_fails_a_planted_plugin() {
    let dir = Scratch::new("plugins-run");
    let clean = case!("clean");
    fs::create_dir_all(dir.join("crates/desktop/src")).expect("create the crate dirs");
    fs::write(dir.join("Cargo.toml"), &clean.root).expect("write the root manifest");
    fs::write(dir.join("crates/desktop/src/lib.rs"), "").expect("write lib.rs");
    fs::write(dir.join(DESKTOP_PATH), &clean.desktop).expect("write the desktop manifest");
    let ctx = Context {
        members: workspace::load_members(&dir).expect("cargo metadata runs"),
        ..desktop_context(&dir)
    };
    assert_eq!(run(&ctx), Vec::new());
    let planted = format!("{}tauri-plugin-fs = \"2\"\n", clean.desktop);
    fs::write(dir.join(DESKTOP_PATH), &planted).expect("write the desktop manifest");
    assert_eq!(
        run(&ctx),
        vec![violation(
            DESKTOP_PATH,
            line_of(&planted, "tauri-plugin-fs"),
            "`tauri-plugin-fs` is not in the fixed plugin set",
            PLUGIN_FIX,
        )]
    );
}

#[test]
fn run_fails_once_when_the_desktop_crate_is_not_a_member() {
    let dir = Scratch::new("plugins-no-member");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
    };
    let found = run(&ctx);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].path.as_str(), found[0].line), ("Cargo.toml", 1));
}
