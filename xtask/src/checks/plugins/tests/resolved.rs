//! Planted cases for the graph rule of the `plugins` check, with the graph on (`Repo::resolved`).
//! Every registry dependency is patched (`[patch.crates-io]`) to a path crate under `vendor/`, so
//! `cargo generate-lockfile --offline` and the check's `cargo tree --locked --offline` resolve the
//! desktop crate's graph without the network, while `cargo metadata --no-deps` still sees registry dependencies,
//! not path ones (a path dependency outside the workspace fails another rule of the check).

use super::{FIXED_PLUGINS, write_file};
use crate::checks::planted::Repo;
use crate::scratch::Scratch;

/// The scratch workspace `planted-plugins-<name>`: the desktop crate with `tauri`,
/// `tauri-runtime` and the fixed plugins, plus an optional `mid` behind its feature `extra`;
/// `mid`'s manifest body is `mid_body`. `check` runs on it with the graph on.
fn optional_mid(name: &str, mid_body: &str) -> Repo {
    let dir = Scratch::new(&format!("planted-plugins-{name}"));
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
        "[package]\nname = \"clauseo-desktop\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\n{inherited}mid = {{ version = \"0.1\", optional = true }}\n\n[features]\nextra = [\"dep:mid\"]\n"
    );
    let vendored: Vec<(&str, &str, &str)> = fixed
        .iter()
        .map(|package| (*package, "2.0.0", ""))
        .chain([("mid", "0.1.0", mid_body), ("tauri-plugin-zz", "2.0.0", "")])
        .collect();
    let patches = vendored
        .iter()
        .map(|(p, _, _)| format!("{p} = {{ path = \"vendor/{p}\" }}\n"))
        .collect::<Vec<_>>()
        .concat();
    let root = format!(
        "[workspace]\nresolver = \"3\"\nmembers = [\"crates/*\"]\n\n[workspace.dependencies]\n{requirements}\n[patch.crates-io]\n{patches}"
    );
    for (package, version, body) in vendored {
        let manifest = format!(
            "[package]\nname = \"{package}\"\nversion = \"{version}\"\nedition = \"2024\"\n\n{body}"
        );
        write_file(&dir, &format!("vendor/{package}/Cargo.toml"), &manifest);
        write_file(&dir, &format!("vendor/{package}/src/lib.rs"), "");
    }
    write_file(&dir, "Cargo.toml", &root);
    write_file(&dir, "crates/desktop/Cargo.toml", &desktop);
    write_file(&dir, "crates/desktop/src/lib.rs", "");
    Repo::resolved(dir)
}

pub(in crate::checks::plugins) fn plugin_behind_an_optional_desktop_feature() -> Repo {
    optional_mid(
        "optional_feature_plugin",
        "[dependencies]\ntauri-plugin-zz = \"2\"\n",
    )
}

pub(in crate::checks::plugins) fn optional_desktop_feature_without_a_plugin_passes() -> Repo {
    optional_mid("optional_feature_clean", "")
}
