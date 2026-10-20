//! Planted cases for the graph rule of the `plugins` check, with the graph on (`Repo::resolved`).
//! crates.io is replaced (`.cargo/config.toml` `[source]`) by the directory source `vendor/`,
//! which holds every registry dependency, so `cargo generate-lockfile --offline` and the check's
//! `cargo tree --locked --offline` resolve the desktop crate's graph without the network, while
//! `cargo metadata --no-deps` still sees registry dependencies, not path ones (a path dependency
//! outside the workspace fails another rule of the check). Not `[patch]`: a patch of `tauri` or a
//! fixed plugin fails the check's patch rule.

use super::{FIXED_PLUGINS, member, write_file};
use crate::checks::planted::Repo;
use crate::scratch::Scratch;

/// The scratch workspace `planted-plugins-<name>`: the desktop crate with `tauri`,
/// `tauri-runtime` and the fixed plugins in `[dependencies]` (lines 7 to 16), then `tail` from
/// line 17; one member per `(dir, manifest)` in `members`; and, patched in under `vendor/`, the
/// fixed crates (empty) and one crate per `(package, version, body)` in `extra`, as the
/// directory source that replaces crates.io. `check` runs on it with the graph on.
fn patched(name: &str, tail: &str, members: &[(&str, &str)], extra: &[(&str, &str, &str)]) -> Repo {
    let dir = Scratch::new(&format!("planted-plugins-{name}"));
    write_vendored(&dir, tail, "", members, extra);
    Repo::resolved(dir)
}

/// Writes into `dir` the workspace `patched` describes, with `root_tail` after the root
/// manifest's `[workspace.dependencies]` (lines 6 to 15), from line 16.
fn write_vendored(
    dir: &Scratch,
    tail: &str,
    root_tail: &str,
    members: &[(&str, &str)],
    extra: &[(&str, &str, &str)],
) {
    let fixed: Vec<&str> = ["tauri", "tauri-runtime"]
        .into_iter()
        .chain(FIXED_PLUGINS)
        .collect();
    let requirements = fixed
        .iter()
        .map(|p| format!("{p} = \"2\"\n"))
        .collect::<Vec<_>>()
        .concat();
    let inherited = fixed
        .iter()
        .map(|p| format!("{p}.workspace = true\n"))
        .collect::<Vec<_>>()
        .concat();
    let desktop = format!(
        "[package]\nname = \"clauseo-desktop\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\n{inherited}{tail}"
    );
    let vendored: Vec<(&str, &str, &str)> = fixed
        .iter()
        .map(|package| (*package, "2.0.0", ""))
        .chain(extra.iter().copied())
        .collect();
    let root = format!(
        "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\n{requirements}{root_tail}"
    );
    let config = "[source.crates-io]\nreplace-with = \"vendored\"\n\n[source.vendored]\ndirectory = \"vendor\"\n";
    write_file(dir, ".cargo/config.toml", config);
    for (package, version, body) in vendored {
        let manifest = format!(
            "[package]\nname = \"{package}\"\nversion = \"{version}\"\nedition = \"2024\"\n\n{body}"
        );
        write_file(dir, &format!("vendor/{package}/Cargo.toml"), &manifest);
        write_file(dir, &format!("vendor/{package}/src/lib.rs"), "");
        // A directory source lists each crate's file checksums; none listed, none checked.
        let checksums = "{\"files\":{},\"package\":null}";
        write_file(
            dir,
            &format!("vendor/{package}/.cargo-checksum.json"),
            checksums,
        );
    }
    write_file(dir, "Cargo.toml", &root);
    for (crate_dir, manifest) in [("crates/desktop", desktop.as_str())].iter().chain(members) {
        write_file(dir, &format!("{crate_dir}/Cargo.toml"), manifest);
        write_file(dir, &format!("{crate_dir}/src/lib.rs"), "");
    }
}

/// The plugin outside the set, patched in as `tauri-plugin-zz` 2.0.0.
const ZZ: (&str, &str, &str) = ("tauri-plugin-zz", "2.0.0", "");

/// The body of a crate that depends on `tauri-plugin-zz`.
const BRINGS_ZZ: &str = "[dependencies]\ntauri-plugin-zz = \"2\"\n";

/// The workspace with an optional `mid` 0.1.0 behind the desktop feature `extra` (line 17);
/// `mid`'s manifest body is `mid_body`.
fn optional_mid(name: &str, mid_body: &str) -> Repo {
    patched(
        name,
        "mid = { version = \"0.1\", optional = true }\n\n[features]\nextra = [\"dep:mid\"]\n",
        &[],
        &[("mid", "0.1.0", mid_body), ZZ],
    )
}

pub(in crate::checks::plugins) fn plugin_behind_an_optional_desktop_feature() -> Repo {
    optional_mid("optional_feature_plugin", BRINGS_ZZ)
}

pub(in crate::checks::plugins) fn optional_desktop_feature_without_a_plugin_passes() -> Repo {
    optional_mid("optional_feature_clean", "")
}

/// `tauri-plugin-zz` as a build dependency of the desktop crate, at line 19: build edges are in
/// the graph, though the `[dependencies]` rules skip them.
pub(in crate::checks::plugins) fn plugin_as_a_desktop_build_dependency() -> Repo {
    patched(
        "build_dependency_plugin",
        "\n[build-dependencies]\ntauri-plugin-zz = \"2\"\n",
        &[],
        &[ZZ],
    )
}

/// The member `clauseo-core` (line 17 of the desktop manifest) depends on `mid`, which pulls in
/// `tauri-plugin-zz`: no Tauri crate in core's own manifest, but in the desktop crate's graph.
pub(in crate::checks::plugins) fn plugin_through_another_member() -> Repo {
    let core = member("clauseo-core", "[dependencies]\nmid = \"0.1\"\n");
    patched(
        "member_plugin",
        "clauseo-core = { path = \"../core\" }\n",
        &[("crates/core", core.as_str())],
        &[("mid", "0.1.0", BRINGS_ZZ), ZZ],
    )
}

/// `evil` (line 17) depends on `tauri`, so it can register a plugin, though its name is no
/// `tauri-plugin-*`.
pub(in crate::checks::plugins) fn plugin_by_another_name() -> Repo {
    patched(
        "plugin_by_another_name",
        "evil = \"0.1\"\n",
        &[],
        &[("evil", "0.1.0", "[dependencies]\ntauri = \"2\"\n")],
    )
}

/// The hole the patch rule closes: root `[patch.crates-io]` (line 18) swaps the fixed
/// `tauri-plugin-dialog` for `forks/tauri-plugin-dialog`, which pulls in `tauri-plugin-zz`. The
/// graph rule trusts what a fixed plugin pulls in, so only the patch rule fails it.
pub(in crate::checks::plugins) fn patched_fixed_plugin_brings_a_plugin() -> Repo {
    let dir = Scratch::new("planted-plugins-patched_fixed_plugin");
    let root_tail =
        "\n[patch.crates-io]\ntauri-plugin-dialog = { path = \"forks/tauri-plugin-dialog\" }\n";
    write_vendored(&dir, "", root_tail, &[], &[ZZ]);
    let fork = format!(
        "[package]\nname = \"tauri-plugin-dialog\"\nversion = \"2.0.0\"\nedition = \"2024\"\n\n{BRINGS_ZZ}"
    );
    write_file(&dir, "forks/tauri-plugin-dialog/Cargo.toml", &fork);
    write_file(&dir, "forks/tauri-plugin-dialog/src/lib.rs", "");
    Repo::resolved(dir)
}
