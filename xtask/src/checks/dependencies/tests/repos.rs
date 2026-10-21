//! The builders of the dependencies planted cases: a scratch workspace with an `ARCHITECTURE.md`
//! and path crates. The crates under `vendor/` (excluded from the workspace) stand in for registry
//! crates such as `tauri`, so the check's own `cargo tree --locked --offline` resolves each
//! crate's graph without the network, once `Repo::resolved` has written the lockfile. The shared
//! helpers are here; the builders are by topic in `repos/`.

mod bypass;
mod manifests;
mod members;
mod names;
mod table;
mod tree;

pub(super) use bypass::*;
pub(super) use manifests::*;
pub(super) use members::*;
pub(super) use names::*;
pub(super) use table::*;
pub(super) use tree::*;

use crate::checks::planted::{self, Repo};
use crate::scratch::Scratch;

/// The header of a scratch `ARCHITECTURE.md`; its first row lands on line 7.
pub(super) const TABLE_HEAD: &str = "# Architecture\n\n## Crates\n\n| Crate | Owns | May depend on | Never depends on | Status |\n|---|---|---|---|---|\n";

/// The protocol row, on line 7 when it comes first.
pub(super) const PROTOCOL_ROW: &str =
    "| `protocol` | events | serde | tauri, tokio, fs | exists |\n";

/// A scratch workspace: one `(dir, manifest)` per crate (members are those under `crates/`), the
/// root `[workspace.dependencies]` body `workspace_deps`, and the Crates table `rows`.
pub(super) fn workspace(
    name: &str,
    rows: &str,
    workspace_deps: &str,
    crates: &[(String, String)],
) -> Scratch {
    let members: Vec<String> = crates
        .iter()
        .filter(|(crate_dir, _)| crate_dir.starts_with("crates/"))
        .map(|(crate_dir, _)| format!("\"{crate_dir}\""))
        .collect();
    let root = format!(
        "[workspace]\nresolver = \"3\"\nmembers = [{}]\nexclude = [\"vendor\"]\n\n[workspace.dependencies]\n{workspace_deps}",
        members.join(", ")
    );
    let mut files = vec![
        ("Cargo.toml".to_owned(), root),
        ("ARCHITECTURE.md".to_owned(), format!("{TABLE_HEAD}{rows}")),
    ];
    for (crate_dir, manifest) in crates {
        files.push((format!("{crate_dir}/src/lib.rs"), String::new()));
        files.push((format!("{crate_dir}/Cargo.toml"), manifest.clone()));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, text)| (path.as_str(), text.as_str()))
        .collect();
    planted::scratch(&format!("dependencies-{name}"), &files)
}

/// A manifest for package `name`, followed by `body`: its sixth line is the first of `body`'s.
pub(super) fn manifest(name: &str, body: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{body}")
}

/// A vendored crate `name` (outside the workspace) with `body`.
pub(super) fn vendor(name: &str, body: &str) -> (String, String) {
    (format!("vendor/{name}"), manifest(name, body))
}

/// The stand-in for the real `tauri`.
pub(super) fn tauri() -> (String, String) {
    vendor("tauri", "")
}

/// The workspace in `dir`, which the check reads with its resolved graph on.
pub(super) fn run_on(dir: Scratch) -> Repo {
    Repo::resolved(dir)
}

/// A workspace whose only member is `clauseo-protocol` with `body`, plus `vendored` crates.
fn protocol(name: &str, body: &str, vendored: Vec<(String, String)>) -> Repo {
    let mut crates = vec![(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", body),
    )];
    crates.extend(vendored);
    run_on(workspace(name, PROTOCOL_ROW, "", &crates))
}
