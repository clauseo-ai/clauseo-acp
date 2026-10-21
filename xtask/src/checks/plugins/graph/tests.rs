//! Tests for the graph rules of the `plugins` check. Each case is a scratch workspace of path
//! crates named like the registry crates they stand for (`wry`, `tauri`, `tauri-plugin-*`), so
//! the real `cargo tree --offline` resolves the graph without a lockfile or the network.

use super::*;
use crate::checks;
use crate::scratch::Scratch;
use crate::workspace;

const DESKTOP_PATH: &str = "crates/desktop/Cargo.toml";

/// A scratch workspace with the desktop manifest `desktop` and one path crate per
/// `(name, manifest body)` in `crates`, each under `vendor/<name>`.
fn workspace(name: &str, desktop: &str, crates: &[(&str, &str)]) -> Scratch {
    let dir = Scratch::new(&format!("plugins-graph-{name}"));
    let root = "[workspace]\nresolver = \"3\"\nmembers = [\"crates/desktop\"]\n";
    Scratch::write(dir.join("Cargo.toml"), root).expect("write the root manifest");
    let mut all = vec![("crates/desktop".to_owned(), desktop.to_owned())];
    for (crate_name, body) in crates {
        let manifest = format!(
            "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{body}"
        );
        all.push((format!("vendor/{crate_name}"), manifest));
    }
    for (crate_dir, manifest) in all {
        Scratch::create_dir_all(dir.join(&crate_dir).join("src")).expect("create the crate dirs");
        Scratch::write(dir.join(&crate_dir).join("src/lib.rs"), "").expect("write lib.rs");
        Scratch::write(dir.join(&crate_dir).join("Cargo.toml"), manifest)
            .expect("write a manifest");
    }
    dir
}

/// The graph violations of the workspace in `dir`, whose desktop manifest is `desktop`.
fn violations_in(dir: &Scratch, desktop: &str) -> Vec<Violation> {
    let tree = checks::graph::load_tree(dir, checks::DESKTOP_PACKAGE, &["--offline"])
        .expect("cargo tree runs");
    let members = workspace::load(dir).expect("cargo metadata runs");
    let desktop_member = members
        .iter()
        .find(|member| member.name == checks::DESKTOP_PACKAGE)
        .expect("the desktop crate is a member");
    violations(
        &tree,
        &Desktop {
            path: DESKTOP_PATH,
            text: desktop,
            deps: &desktop_member.dependencies,
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

/// The desktop crate declares `s` twice, in `[dependencies]` with `f1` and in
/// `[build-dependencies]` with `f2`. `s`'s optional `tauri-plugin-zz` sits behind the features
/// `s_features` gives.
fn declared_twice(name: &str, s_features: &str) -> (Scratch, String) {
    let desktop = desktop(
        "[dependencies]\ns = { path = \"../../vendor/s\", features = [\"f1\"] }\n\n[build-dependencies]\ns = { path = \"../../vendor/s\", features = [\"f2\"] }\n",
    );
    let s = format!(
        "[dependencies]\ntauri-plugin-zz = {{ path = \"../tauri-plugin-zz\", optional = true }}\n\n[features]\n{s_features}\n"
    );
    let ws = workspace(name, &desktop, &[("s", &s), ("tauri-plugin-zz", "")]);
    (ws, desktop)
}

#[test]
fn a_plugin_behind_only_the_build_declaration_blames_the_build_line() {
    let (ws, desktop) = declared_twice(
        "declared-twice-build",
        "f1 = []\nf2 = [\"dep:tauri-plugin-zz\"]",
    );
    assert_eq!(
        violations_in(&ws, &desktop),
        vec![violation(
            DESKTOP_PATH,
            line_of(&desktop, "features = [\"f2\"]"),
            "`tauri-plugin-zz` 0.1.0 is in the desktop crate's tree through `s`, which is not a fixed plugin",
            TREE_FIX,
        )]
    );
}

#[test]
fn a_plugin_behind_both_declarations_names_both_lines() {
    let (ws, desktop) = declared_twice(
        "declared-twice-both",
        "f1 = [\"dep:tauri-plugin-zz\"]\nf2 = [\"dep:tauri-plugin-zz\"]",
    );
    assert_eq!(
        violations_in(&ws, &desktop),
        vec![violation(
            DESKTOP_PATH,
            line_of(&desktop, "features = [\"f1\"]"),
            "`tauri-plugin-zz` 0.1.0 is in the desktop crate's tree through `s`, which is not a fixed plugin. `s` is declared at lines 7 and 10 of crates/desktop/Cargo.toml, and each declaration brings `tauri-plugin-zz` in",
            "remove `s` from lines 7 and 10 of crates/desktop/Cargo.toml: only the fixed plugins and what they pull in may be in the desktop crate's tree",
        )]
    );
}

/// `s` in `[dependencies]` with `f1` and in a `cfg(windows)` table with `f3`, where only `f3`
/// brings the plugin in. Both are normal lines, so cargo builds them as one package.
#[test]
fn a_plugin_behind_one_of_two_normal_declarations_says_one_or_more_bring_it_in() {
    let desktop = desktop(
        "[dependencies]\ns = { path = \"../../vendor/s\", features = [\"f1\"] }\n\n[target.'cfg(windows)'.dependencies]\ns = { path = \"../../vendor/s\", features = [\"f3\"] }\n",
    );
    let s = "[dependencies]\ntauri-plugin-zz = { path = \"../tauri-plugin-zz\", optional = true }\n\n[features]\nf1 = []\nf3 = [\"dep:tauri-plugin-zz\"]\n";
    let ws = workspace(
        "declared-twice-one-section",
        &desktop,
        &[("s", s), ("tauri-plugin-zz", "")],
    );
    assert_eq!(
        violations_in(&ws, &desktop),
        vec![violation(
            DESKTOP_PATH,
            line_of(&desktop, "features = [\"f1\"]"),
            "`tauri-plugin-zz` 0.1.0 is in the desktop crate's tree through `s`, which is not a fixed plugin. `s` is declared at lines 7 and 10 of crates/desktop/Cargo.toml. Cargo builds these declarations as one package, and one or more of them brings `tauri-plugin-zz` in",
            "remove `s` from lines 7 and 10 of crates/desktop/Cargo.toml: only the fixed plugins and what they pull in may be in the desktop crate's tree",
        )]
    );
}

/// The desktop crate declares `s1 = { package = "s", path = "../../vendor/s1" }` at line 7 and
/// `s2`, the same for `vendor/s2`, at line 10. They are two packages named `s`, 1.0.0 and
/// 2.0.0. Each item of `bodies` is the manifest body of one of them.
fn two_paths_of_one_name(name: &str, bodies: [&str; 2]) -> Vec<Violation> {
    let desktop = desktop(
        "[dependencies]\ns1 = { package = \"s\", path = \"../../vendor/s1\" }\n\n[target.'cfg(windows)'.dependencies]\ns2 = { package = \"s\", path = \"../../vendor/s2\" }\n",
    );
    let ws = workspace(name, &desktop, &[("tauri-plugin-zz", "")]);
    // A path dependency under the root is a member unless excluded, and two members cannot
    // share a name.
    let root =
        "[workspace]\nresolver = \"3\"\nmembers = [\"crates/desktop\"]\nexclude = [\"vendor\"]\n";
    Scratch::write(ws.join("Cargo.toml"), root).expect("write the root manifest");
    for (s_dir, version, body) in [("s1", "1.0.0", bodies[0]), ("s2", "2.0.0", bodies[1])] {
        let manifest = format!(
            "[package]\nname = \"s\"\nversion = \"{version}\"\nedition = \"2024\"\n\n{body}"
        );
        let dir = ws.join("vendor").join(s_dir);
        Scratch::create_dir_all(dir.join("src")).expect("create the crate dirs");
        Scratch::write(dir.join("src/lib.rs"), "").expect("write lib.rs");
        Scratch::write(dir.join("Cargo.toml"), manifest).expect("write a manifest");
    }
    violations_in(&ws, &desktop)
}

#[test]
fn two_packages_of_one_name_are_blamed_by_their_own_keys() {
    let plugin = "[dependencies]\ntauri-plugin-zz = { path = \"../tauri-plugin-zz\" }\n";
    let message = "`tauri-plugin-zz` 0.1.0 is in the desktop crate's tree through `s`, which is not a fixed plugin";
    assert_eq!(
        two_paths_of_one_name("one-name-s2", ["", plugin]),
        vec![violation(DESKTOP_PATH, 10, message, TREE_FIX)]
    );
    assert_eq!(
        two_paths_of_one_name("one-name-both", [plugin, plugin]),
        vec![violation(
            DESKTOP_PATH,
            7,
            format!(
                "{message}. `s1` and `s2` are declared at lines 7 and 10 of crates/desktop/Cargo.toml, and each declaration brings `tauri-plugin-zz` in"
            ),
            "remove `s1` and `s2` from lines 7 and 10 of crates/desktop/Cargo.toml: only the fixed plugins and what they pull in may be in the desktop crate's tree",
        )]
    );
}

#[test]
fn a_clean_graph_passes() {
    let desktop = desktop("[dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n");
    let ws = workspace("clean", &desktop, &[("tauri", "")]);
    assert_eq!(violations_in(&ws, &desktop), Vec::new());
}

#[test]
fn a_plugin_behind_an_optional_dependency_of_a_desktop_feature_fails() {
    let desktop = desktop(
        "[dependencies]\nmid = { path = \"../../vendor/mid\", optional = true }\n\n[features]\nextra = [\"dep:mid\"]\n",
    );
    let ws = workspace(
        "optional",
        &desktop,
        &[
            (
                "mid",
                "[dependencies]\ntauri-plugin-zz = { path = \"../tauri-plugin-zz\" }\n",
            ),
            ("tauri-plugin-zz", ""),
        ],
    );
    assert_eq!(
        violations_in(&ws, &desktop),
        vec![violation(
            DESKTOP_PATH,
            line_of(&desktop, "mid = "),
            "`tauri-plugin-zz` 0.1.0 is in the desktop crate's tree through `mid`, which is not a fixed plugin",
            TREE_FIX,
        )]
    );
}

/// A vendored `spec` whose optional `tauri-plugin-zz` sits behind its `zz` feature, used by the
/// desktop crate as `desktop_spec`; the workspace member `other` uses it as `other_spec`, and
/// the desktop crate's dev-dependency `devonly` turns `zz` on.
fn spec_feature(name: &str, desktop_spec: &str, other_spec: &str) -> Vec<Violation> {
    let desktop = desktop(&format!(
        "[dependencies]\n{desktop_spec}\n\n[dev-dependencies]\ndevonly = {{ path = \"../../vendor/devonly\" }}\n"
    ));
    let spec_line = "spec = { path = \"../spec\", features = [\"zz\"] }";
    let ws = workspace(
        name,
        &desktop,
        &[
            (
                "spec",
                "[dependencies]\ntauri-plugin-zz = { path = \"../tauri-plugin-zz\", optional = true }\n\n[features]\nzz = [\"dep:tauri-plugin-zz\"]\n",
            ),
            ("tauri-plugin-zz", ""),
            ("devonly", &format!("[dependencies]\n{spec_line}\n")),
            ("other", &format!("[dependencies]\n{other_spec}\n")),
        ],
    );
    let root = "[workspace]\nresolver = \"3\"\nmembers = [\"crates/desktop\", \"vendor/other\"]\n";
    Scratch::write(ws.join("Cargo.toml"), root).expect("write the root manifest");
    violations_in(&ws, &desktop)
}

#[test]
fn a_plugin_behind_a_feature_only_another_member_or_a_dev_dependency_turns_on_passes() {
    let found = spec_feature(
        "other-member",
        "spec = { path = \"../../vendor/spec\" }",
        "spec = { path = \"../spec\", features = [\"zz\"] }",
    );
    assert_eq!(found, Vec::new());
}

#[test]
fn a_plugin_behind_a_feature_the_desktop_crate_turns_on_fails() {
    let desktop_spec = "spec = { path = \"../../vendor/spec\", features = [\"zz\"] }";
    let found = spec_feature(
        "desktop-feature",
        desktop_spec,
        "spec = { path = \"../spec\" }",
    );
    let desktop = desktop(&format!(
        "[dependencies]\n{desktop_spec}\n\n[dev-dependencies]\ndevonly = {{ path = \"../../vendor/devonly\" }}\n"
    ));
    assert_eq!(
        found,
        vec![violation(
            DESKTOP_PATH,
            line_of(&desktop, "spec = "),
            "`tauri-plugin-zz` 0.1.0 is in the desktop crate's tree through `spec`, which is not a fixed plugin",
            TREE_FIX,
        )]
    );
}
