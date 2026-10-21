//! Planted cases for the graph rule of the `plugins` check, with the graph on
//! (`Repo::resolved_in`). The scratch folder holds the workspace in `repo/` and the directory
//! source `vendor/`, with every registry dependency; `.cargo/config.toml` of the scratch folder,
//! not of the workspace, replaces crates.io with `vendor/` (`[source]`). Cargo reads the config of
//! every parent folder, so `cargo generate-lockfile --offline` and the check's
//! `cargo tree --locked --offline` resolve the desktop crate's graph without the network, while
//! the workspace keeps no config of its own. `cargo metadata --no-deps` still sees registry
//! dependencies, not path ones (a path dependency outside the workspace fails another rule of the
//! check).

use super::*;
use crate::checks::planted::Repo;

/// The folder of the scratch folder that holds the workspace.
const REPO: &str = "repo";

/// The config that replaces crates.io with the directory source `vendor/` beside it.
const VENDORED: &str = "[source.crates-io]\nreplace-with = \"vendored\"\n\n[source.vendored]\ndirectory = \"vendor\"\n";

/// The scratch workspace `planted-plugins-<name>`: the desktop crate with `tauri`,
/// `tauri-runtime` and the fixed plugins in `[dependencies]` (lines 7 to 16), then `tail` from
/// line 17; one member per `(dir, manifest)` in `members`; and, beside it under `vendor/`, the
/// fixed crates (empty) and one crate per `(package, version, body)` in `extra`, as the
/// directory source that replaces crates.io. `check` runs on it with the graph on.
fn patched(name: &str, tail: &str, members: &[(&str, &str)], extra: &[(&str, &str, &str)]) -> Repo {
    let files = vendored_files(tail, "", members, extra);
    Repo::resolved_in(scratch_of(name, &files), REPO)
}

/// The files of the workspace `patched` describes, in `repo/` of the scratch folder, with
/// `root_tail` after the root manifest's `[workspace.dependencies]` (lines 6 to 15), from line 16.
fn vendored_files(
    tail: &str,
    root_tail: &str,
    members: &[(&str, &str)],
    extra: &[(&str, &str, &str)],
) -> Files {
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
    let mut files = vec![file(".cargo/config.toml", VENDORED)];
    files.extend(source_files("vendor", &vendored));
    files.push(file(&format!("{REPO}/Cargo.toml"), &root));
    for (crate_dir, manifest) in [("crates/desktop", desktop.as_str())].iter().chain(members) {
        files.push(file(&format!("{REPO}/{crate_dir}/Cargo.toml"), manifest));
        files.push(file(&format!("{REPO}/{crate_dir}/src/lib.rs"), ""));
    }
    files
}

/// The files of a directory source at `source`, with one crate per `(package, version, body)`.
fn source_files(source: &str, crates: &[(&str, &str, &str)]) -> Files {
    let mut files = Vec::new();
    for (package, version, body) in crates {
        let manifest = format!(
            "[package]\nname = \"{package}\"\nversion = \"{version}\"\nedition = \"2024\"\n\n{body}"
        );
        files.push(file(&format!("{source}/{package}/Cargo.toml"), &manifest));
        files.push(file(&format!("{source}/{package}/src/lib.rs"), ""));
        // A directory source lists each crate's file checksums; none listed, none checked.
        let checksums = "{\"files\":{},\"package\":null}";
        files.push(file(
            &format!("{source}/{package}/.cargo-checksum.json"),
            checksums,
        ));
    }
    files
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

pub(in crate::checks::plugins::tests) fn plugin_behind_an_optional_desktop_feature() -> Repo {
    optional_mid("optional_feature_plugin", BRINGS_ZZ)
}

pub(in crate::checks::plugins::tests) fn optional_desktop_feature_without_a_plugin_passes() -> Repo
{
    optional_mid("optional_feature_clean", "")
}
