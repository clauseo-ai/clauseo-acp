//! Tests for the `plugins` check. Each case is a tiny scratch workspace (a root `Cargo.toml`,
//! member manifests and empty `src/lib.rs` files) that `run` reads through the real
//! `cargo metadata --no-deps`, so cargo itself resolves the renames and inheritance under test.
//! They skip the graph rules, which need a lockfile: `graph/tests.rs` covers those.

use super::*;
use crate::checks::tests::desktop_context;
use crate::scratch::Scratch;
use std::fs;

const DESKTOP_PATH: &str = "crates/desktop/Cargo.toml";

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

/// A scratch workspace and the files written into it.
struct Workspace {
    dir: Scratch,
    root: String,
    desktop: String,
}

impl Workspace {
    /// A workspace named `name` (unique, as `Scratch` requires) with the root manifest `root`, the
    /// desktop manifest `desktop` and one crate per `(dir, manifest)` in `others`.
    fn new(name: &str, root: &str, desktop: &str, others: &[(&str, &str)]) -> Workspace {
        let dir = Scratch::new(&format!("plugins-{name}"));
        fs::write(dir.join("Cargo.toml"), root).expect("write the root manifest");
        for (crate_dir, manifest) in [("crates/desktop", desktop)].iter().chain(others) {
            fs::create_dir_all(dir.join(crate_dir).join("src")).expect("create the crate dirs");
            fs::write(dir.join(crate_dir).join("src/lib.rs"), "").expect("write lib.rs");
            fs::write(dir.join(crate_dir).join("Cargo.toml"), manifest).expect("write a manifest");
        }
        Workspace {
            dir,
            root: root.to_owned(),
            desktop: desktop.to_owned(),
        }
    }

    /// The clean workspace with `from` replaced by `to` in the desktop manifest.
    fn desktop_with(name: &str, from: &str, to: &str) -> Workspace {
        assert!(
            DESKTOP.contains(from),
            "the desktop manifest contains {from}"
        );
        Workspace::new(name, ROOT, &DESKTOP.replacen(from, to, 1), &[])
    }

    /// The violations of `inspection`.
    fn violations(&self) -> Vec<Violation> {
        self.inspection().violations
    }

    /// `run` on the workspace without the resolved graph, with the members `cargo metadata` lists.
    fn inspection(&self) -> Inspection {
        let ctx = Context {
            members: workspace::load_members(&self.dir).expect("cargo metadata runs"),
            load_resolved_graph: false,
            ..desktop_context(&self.dir)
        };
        check(&ctx, None)
    }
}

/// Line number of the first line of `text` that contains `needle`.
fn line_of(text: &str, needle: &str) -> u32 {
    let index = text
        .lines()
        .position(|line| line.contains(needle))
        .expect("the needle is in the manifest");
    checks::line_at(index)
}

#[test]
fn the_fixed_plugin_set_passes_and_counts_each_direct_dependency() {
    assert_eq!(
        Workspace::new("clean", ROOT, DESKTOP, &[]).inspection(),
        Inspection::new(10, Vec::new())
    );
}

#[test]
fn an_optional_fixed_plugin_behind_a_feature_fails() {
    let desktop = DESKTOP.replacen(
        "tauri-plugin-dialog.workspace = true\n",
        "tauri-plugin-dialog = { workspace = true, optional = true }\n",
        1,
    );
    let desktop = format!("{desktop}\n[features]\ndefault = [\"tauri-plugin-dialog\"]\n");
    let ws = Workspace::new("optional", ROOT, &desktop, &[]);
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "optional = true"),
            "`tauri-plugin-dialog` is optional",
            OPTIONAL_FIX,
        )]
    );
}

#[test]
fn a_plugin_renamed_in_workspace_dependencies_counts_under_its_package() {
    let root = format!("{ROOT}files = {{ package = \"tauri-plugin-fs\", version = \"2\" }}\n");
    let desktop = format!("{DESKTOP}files.workspace = true\n");
    let ws = Workspace::new("renamed-plugin", &root, &desktop, &[]);
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "files.workspace = true"),
            "`tauri-plugin-fs` (as `files`) is not in the fixed plugin set",
            PLUGIN_FIX,
        )]
    );
}

#[test]
fn a_renamed_tauri_with_devtools_and_a_feature_through_its_rename_fail() {
    let root = ROOT.replace(
        "tauri = \"2\"\n",
        "t = { package = \"tauri\", version = \"2\", features = [\"devtools\"] }\n",
    );
    let desktop = format!(
        "{}\n[features]\nasset = [\"t/protocol-asset\"]\n",
        DESKTOP.replace("tauri.workspace = true\n", "t.workspace = true\n")
    );
    let ws = Workspace::new("renamed-tauri", &root, &desktop, &[]);
    assert_eq!(
        ws.violations(),
        vec![
            violation(
                WORKSPACE_MANIFEST,
                line_of(&ws.root, "package = \"tauri\""),
                "`tauri` enables the `devtools` feature",
                FEATURE_FIX,
            ),
            violation(
                DESKTOP_PATH,
                line_of(&ws.desktop, "t/protocol-asset"),
                "`[features]` enables the `tauri` feature `protocol-asset` through `t/protocol-asset`",
                FEATURE_FIX,
            ),
        ]
    );
}

#[test]
fn a_target_plugin_counts_and_a_dev_plugin_does_not() {
    let desktop = format!(
        "{DESKTOP}\n[target.'cfg(windows)'.dependencies]\ntauri-plugin-fs = \"2\"\n\n[dev-dependencies]\ntauri-plugin-shell = \"2\"\n"
    );
    let ws = Workspace::new("target", ROOT, &desktop, &[]);
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "tauri-plugin-fs"),
            "`tauri-plugin-fs` is not in the fixed plugin set",
            PLUGIN_FIX,
        )]
    );
}

#[test]
fn a_missing_plugin_fails_at_the_dependencies_header() {
    let ws = Workspace::desktop_with("missing", "tauri-plugin-os.workspace = true\n", "");
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "[dependencies]"),
            "`tauri-plugin-os` is missing from the fixed plugin set",
            "add it back under [dependencies]: `tauri-plugin-os.workspace = true`",
        )]
    );
}

#[test]
fn devtools_in_the_desktop_entry_fails_at_its_line() {
    let ws = Workspace::desktop_with(
        "devtools",
        "tauri.workspace = true\n",
        "tauri = { workspace = true, features = [\n  \"devtools\",\n] }\n",
    );
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "\"devtools\""),
            "`tauri` enables the `devtools` feature",
            FEATURE_FIX,
        )]
    );
}

#[test]
fn an_optional_tauri_feature_reference_fails() {
    let desktop = DESKTOP.replacen(
        "tauri.workspace = true\n",
        "tauri = { workspace = true, optional = true }\ntauri-x = { path = \"../x\", package = \"helper-x\" }\n",
        1,
    );
    let desktop =
        format!("{desktop}\n[features]\ninspect = [\"tauri?/devtools\", \"tauri-x/devtools\"]\n");
    let x = "[package]\nname = \"helper-x\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[features]\ndevtools = []\n";
    let ws = Workspace::new("optional-tauri", ROOT, &desktop, &[("crates/x", x)]);
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "inspect ="),
            "`[features]` enables the `tauri` feature `devtools` through `tauri?/devtools`",
            FEATURE_FIX,
        )]
    );
}

#[test]
fn a_tauri_runtime_feature_fails_in_the_desktop_or_the_workspace_entry() {
    let ws = Workspace::desktop_with(
        "runtime",
        "tauri-runtime.workspace = true\n",
        "tauri-runtime = { workspace = true, default-features = true, features = [\"devtools\"] }\n",
    );
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "tauri-runtime ="),
            "`tauri-runtime` enables the `devtools` feature",
            RUNTIME_FIX,
        )]
    );
    let root = ROOT.replace(
        "tauri-runtime = \"2\"\n",
        "tauri-runtime = { version = \"2\", features = [\"macos-private-api\"] }\n",
    );
    let ws = Workspace::new("runtime-root", &root, DESKTOP, &[]);
    assert_eq!(
        ws.violations(),
        vec![violation(
            WORKSPACE_MANIFEST,
            line_of(&ws.root, "macos-private-api"),
            "`tauri-runtime` enables the `macos-private-api` feature",
            RUNTIME_FIX,
        )]
    );
}

#[test]
fn another_member_with_tauri_fails_even_through_a_path_dependency() {
    let core = r#"[package]
name = "clauseo-core"
version = "0.0.0"
edition = "2024"

[dependencies]
tauri = { version = "2", features = ["devtools"] }
tauri-plugin-fs = "2"

[target.'cfg(unix)'.dependencies]
rt = { package = "tauri-runtime", version = "2" }

[dev-dependencies]
tauri-plugin-shell = "2"

[build-dependencies]
tauri-plugin-os = "2"
"#;
    let desktop = format!("{DESKTOP}clauseo-core = {{ path = \"../core\" }}\n");
    let ws = Workspace::new("member", ROOT, &desktop, &[("crates/core", core)]);
    let path = "crates/core/Cargo.toml";
    let at =
        |needle: &str, message: &str| violation(path, line_of(core, needle), message, MEMBER_FIX);
    let mut found = ws.violations();
    found.sort_by_key(|v| v.line);
    assert_eq!(
        found,
        vec![
            at("tauri = ", "`clauseo-core` depends on `tauri`"),
            at(
                "tauri-plugin-fs",
                "`clauseo-core` depends on `tauri-plugin-fs`"
            ),
            at(
                "rt = ",
                "`clauseo-core` depends on `tauri-runtime` (as `rt`)"
            ),
        ]
    );
}

#[test]
fn tauri_runtime_wry_with_devtools_in_the_desktop_crate_fails() {
    let desktop = format!(
        "{DESKTOP}tauri-runtime-wry = {{ version = \"2.12.2\", features = [\"devtools\"] }}\n"
    );
    let ws = Workspace::new("runtime-wry", ROOT, &desktop, &[]);
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "tauri-runtime-wry"),
            "`tauri-runtime-wry` is a Tauri crate outside the allowed set",
            FAMILY_FIX,
        )]
    );
}

#[test]
fn wry_in_the_desktop_crate_fails() {
    let desktop = format!(
        "{DESKTOP}\n[target.'cfg(unix)'.dependencies]\nwry = {{ version = \"0.53\", features = [\"devtools\"] }}\n"
    );
    let ws = Workspace::new("wry", ROOT, &desktop, &[]);
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "wry = "),
            "`wry` is a Tauri crate outside the allowed set",
            FAMILY_FIX,
        )]
    );
}

#[test]
fn tauri_build_as_a_build_dependency_passes() {
    let desktop = format!("{DESKTOP}\n[build-dependencies]\ntauri-build = \"2\"\n");
    let ws = Workspace::new("tauri-build", ROOT, &desktop, &[]);
    assert_eq!(ws.violations(), Vec::new());
}

#[test]
fn another_member_with_wry_fails() {
    let core = "[package]\nname = \"clauseo-core\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nwry = \"0.53\"\n";
    let ws = Workspace::new("member-wry", ROOT, DESKTOP, &[("crates/core", core)]);
    assert_eq!(
        ws.violations(),
        vec![violation(
            "crates/core/Cargo.toml",
            line_of(core, "wry = "),
            "`clauseo-core` depends on `wry`",
            MEMBER_FIX,
        )]
    );
}

#[test]
fn a_path_dependency_on_an_excluded_crate_fails() {
    let root = ROOT.replace(
        "members = [\"crates/*\"]\n",
        "members = [\"crates/*\"]\nexclude = [\"evil\"]\n",
    );
    let desktop = format!("{DESKTOP}evil = {{ path = \"../../evil\" }}\n");
    let ws = Workspace::new("excluded", &root, &desktop, &[]);
    fs::create_dir_all(ws.dir.join("evil/src")).expect("create the excluded crate");
    fs::write(ws.dir.join("evil/src/lib.rs"), "").expect("write lib.rs");
    let evil = "[package]\nname = \"evil\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\ntauri-runtime-wry = { version = \"2\", features = [\"devtools\"] }\n";
    fs::write(ws.dir.join("evil/Cargo.toml"), evil).expect("write the excluded manifest");
    assert_eq!(
        ws.violations(),
        vec![violation(
            DESKTOP_PATH,
            line_of(&ws.desktop, "evil = "),
            "`clauseo-desktop` depends on `evil` by path, but `evil` is not a workspace member, so these checks and clippy never see it",
            OUTSIDE_FIX,
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
fn run_fails_once_when_the_desktop_crate_is_not_a_member() {
    let dir = Scratch::new("plugins-no-member");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Vec::new(),
        load_resolved_graph: false,
    };
    let found = run(&ctx).violations;
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].path.as_str(), found[0].line), ("Cargo.toml", 1));
}
