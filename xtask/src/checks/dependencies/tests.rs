//! Tests for the `dependencies` check. Each case is a scratch workspace with an `ARCHITECTURE.md`
//! and path crates; crates under `vendor/` (excluded from the workspace) stand in for registry
//! crates such as `tauri`, so the real `cargo tree --offline` resolves each crate's graph without
//! the network. With no lockfile, cargo writes one inside the scratch dir.

pub(super) mod planted;

use super::*;
use crate::checks::and_list;
use crate::scratch::Scratch;
use crate::workspace;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) mod names;
pub(super) mod table;
pub(super) mod tree;

/// Numbers scratch dirs, so a case run twice in one process never shares a dir.
static NEXT: AtomicUsize = AtomicUsize::new(0);

/// The header of a scratch `ARCHITECTURE.md`; its first row lands on line 7.
const TABLE_HEAD: &str = "# Architecture\n\n## Crates\n\n| Crate | Owns | May depend on | Never depends on | Status |\n|---|---|---|---|---|\n";

/// The protocol row, on line 7 when it comes first.
const PROTOCOL_ROW: &str = "| `protocol` | events | serde | tauri, tokio, fs | exists |\n";

/// A scratch workspace: one `(dir, manifest)` per crate (members are those under `crates/`), the
/// root `[workspace.dependencies]` body `workspace_deps`, and the Crates table `rows`.
fn workspace(name: &str, rows: &str, workspace_deps: &str, crates: &[(String, String)]) -> Scratch {
    let dir = Scratch::new(&format!(
        "dependencies-{name}-{}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let members: Vec<String> = crates
        .iter()
        .filter(|(crate_dir, _)| crate_dir.starts_with("crates/"))
        .map(|(crate_dir, _)| format!("\"{crate_dir}\""))
        .collect();
    let root = format!(
        "[workspace]\nresolver = \"3\"\nmembers = [{}]\nexclude = [\"vendor\"]\n\n[workspace.dependencies]\n{workspace_deps}",
        members.join(", ")
    );
    Scratch::write(dir.join("Cargo.toml"), root).expect("write the root manifest");
    Scratch::write(dir.join("ARCHITECTURE.md"), format!("{TABLE_HEAD}{rows}"))
        .expect("write the table");
    for (crate_dir, manifest) in crates {
        Scratch::create_dir_all(dir.join(crate_dir).join("src")).expect("create the crate dirs");
        Scratch::write(dir.join(crate_dir).join("src/lib.rs"), "").expect("write lib.rs");
        Scratch::write(dir.join(crate_dir).join("Cargo.toml"), manifest).expect("write a manifest");
    }
    dir
}

/// A manifest for package `name`, followed by `body`: its sixth line is the first of `body`'s.
fn manifest(name: &str, body: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n{body}")
}

/// A vendored crate `name` (outside the workspace) with `body`.
fn vendor(name: &str, body: &str) -> (String, String) {
    (format!("vendor/{name}"), manifest(name, body))
}

/// The stand-in for the real `tauri`.
fn tauri() -> (String, String) {
    vendor("tauri", "")
}

/// Runs the check, graph included, on the workspace in `dir`.
fn run_on(dir: &Scratch) -> Inspection {
    let ws = workspace::load(dir).expect("cargo metadata runs");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Ok(ws),
        load_resolved_graph: false,
    };
    check(&ctx, Some(&["--offline"]))
}

/// A workspace whose only member is `clauseo-protocol` with `body`, plus `vendored` crates.
fn protocol(name: &str, body: &str, vendored: Vec<(String, String)>) -> Inspection {
    let mut crates = vec![(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", body),
    )];
    crates.extend(vendored);
    run_on(&workspace(name, PROTOCOL_ROW, "", &crates))
}

pub(super) fn direct() -> Inspection {
    protocol(
        "direct",
        "[dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(super) fn transitive() -> Inspection {
    protocol(
        "transitive",
        "[dependencies]\nmid = { path = \"../../vendor/mid\" }\n",
        vec![
            vendor("mid", "[dependencies]\ninner = { path = \"../inner\" }\n"),
            vendor("inner", "[dependencies]\ntauri = { path = \"../tauri\" }\n"),
            tauri(),
        ],
    )
}

pub(super) fn build_dependency() -> Inspection {
    protocol(
        "build",
        "[build-dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(super) fn renamed() -> Inspection {
    protocol(
        "renamed",
        "[dependencies]\nt = { package = \"tauri\", path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(super) fn target_windows() -> Inspection {
    protocol(
        "windows",
        "[target.'cfg(windows)'.dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(super) fn inherited() -> Inspection {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\ntauri.workspace = true\n",
            ),
        ),
        tauri(),
    ];
    let dir = workspace(
        "inherited",
        PROTOCOL_ROW,
        "tauri = { path = \"vendor/tauri\" }\n",
        &crates,
    );
    run_on(&dir)
}

pub(super) fn optional_feature() -> Inspection {
    protocol(
        "optional",
        "[dependencies]\ntauri = { path = \"../../vendor/tauri\", optional = true }\n\n[features]\nwindow = [\"dep:tauri\"]\n",
        vec![tauri()],
    )
}

pub(super) fn no_row() -> Inspection {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest("clauseo-protocol", ""),
        ),
        ("crates/store".to_owned(), manifest("clauseo-store", "")),
    ];
    run_on(&workspace("no-row", PROTOCOL_ROW, "", &crates))
}

pub(super) fn forbidden_workspace_crate() -> Inspection {
    let rows = "| `host` | sessions | protocol | tauri, agents | exists |\n| `agents` | registry | protocol | tauri | exists |\n";
    let crates = [
        (
            "crates/host".to_owned(),
            manifest(
                "clauseo-host",
                "[dependencies]\nclauseo-agents = { path = \"../agents\" }\n",
            ),
        ),
        ("crates/agents".to_owned(), manifest("clauseo-agents", "")),
    ];
    run_on(&workspace("workspace-crate", rows, "", &crates))
}

pub(super) fn comment_after_header() -> Inspection {
    protocol(
        "comment",
        "[dependencies] # runtime\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(super) fn quoted_key() -> Inspection {
    protocol(
        "quoted",
        "[dependencies]\n'tauri' = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(super) fn table_header() -> Inspection {
    protocol(
        "table-header",
        "[dependencies.tauri]\npath = \"../../vendor/tauri\"\n",
        vec![tauri()],
    )
}

pub(super) fn dash_package_without_row() -> Inspection {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest("clauseo-protocol", ""),
        ),
        (
            "crates/fake_agent".to_owned(),
            manifest("clauseo-fake-agent", ""),
        ),
    ];
    run_on(&workspace("dash-no-row", PROTOCOL_ROW, "", &crates))
}

pub(super) fn workspace_crate_spelled_with_dash() -> Inspection {
    let rows = "| `host` | sessions | protocol | transport_ws | exists |\n| `transport_ws` | dev transport | host | tauri | exists |\n";
    let crates = [
        (
            "crates/host".to_owned(),
            manifest(
                "clauseo-host",
                "[dependencies]\nclauseo-transport-ws = { path = \"../transport_ws\" }\n",
            ),
        ),
        (
            "crates/transport_ws".to_owned(),
            manifest("clauseo-transport-ws", ""),
        ),
    ];
    run_on(&workspace("dash-workspace", rows, "", &crates))
}

pub(super) fn crates_io_spelled_with_underscore() -> Inspection {
    let rows = "| `protocol` | events | serde | async-process | exists |\n";
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\nasync_process = { path = \"../../vendor/async_process\" }\n",
            ),
        ),
        vendor("async_process", ""),
    ];
    run_on(&workspace("dash-crates-io", rows, "", &crates))
}

pub(super) fn std_module_in_never() -> Inspection {
    let rows = "| `protocol` | events | serde | net, tauri | exists |\n";
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    run_on(&workspace("std-module", rows, "", &crates))
}

pub(super) fn dev_dependency_passes() -> Inspection {
    protocol(
        "dev",
        "[dev-dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(super) fn planned_row_without_crate_is_skipped() -> Inspection {
    let rows = format!("{PROTOCOL_ROW}| `host` | sessions | protocol | tauri | planned |\n");
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    run_on(&workspace("planned", &rows, "", &crates))
}

pub(super) fn clean_workspace_passes() -> Inspection {
    protocol(
        "clean",
        "[dependencies]\nserde = { path = \"../../vendor/serde\" }\n",
        vec![vendor("serde", "")],
    )
}

#[test]
fn a_lockfile_behind_the_manifests_fails_with_no_file_line_without_rewriting_it() {
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    let dir = workspace("stale-lock", PROTOCOL_ROW, "", &crates);
    let ws = workspace::load(&dir).expect("cargo metadata runs");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Ok(ws),
        load_resolved_graph: false,
    };
    assert_eq!(check(&ctx, Some(&["--offline"])).violations, Vec::new());
    let lock = fs::read_to_string(dir.join("Cargo.lock")).expect("cargo wrote a lockfile");
    let body = "[dependencies]\nmid = { path = \"../../vendor/mid\" }\n";
    Scratch::write(
        dir.join("crates/protocol/Cargo.toml"),
        manifest("clauseo-protocol", body),
    )
    .expect("write the manifest");
    let (mid_dir, mid) = vendor("mid", "");
    Scratch::create_dir_all(dir.join(&mid_dir).join("src")).expect("create mid");
    Scratch::write(dir.join(&mid_dir).join("src/lib.rs"), "").expect("write lib.rs");
    Scratch::write(dir.join(&mid_dir).join("Cargo.toml"), mid).expect("write mid");
    let found = check(&ctx, Some(&["--locked", "--offline"]));
    assert_eq!(found.violations, Vec::new());
    assert_eq!(found.failures.len(), 1);
    let failure = &found.failures[0];
    assert!(
        failure.fix.contains("cargo update --workspace"),
        "{failure}"
    );
    let after = fs::read_to_string(dir.join("Cargo.lock")).expect("the lockfile is still there");
    assert_eq!(after, lock);
}

#[test]
fn without_the_graph_only_rows_are_matched() {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
            ),
        ),
        tauri(),
    ];
    let dir = workspace("no-graph", PROTOCOL_ROW, "", &crates);
    let ws = workspace::load(&dir).expect("cargo metadata runs");
    let ctx = Context {
        root: dir.to_path_buf(),
        members: Ok(ws),
        load_resolved_graph: false,
    };
    assert_eq!(run(&ctx), Inspection::new(1, Vec::new()));
}

#[test]
fn a_package_outside_the_naming_rule_is_told_to_rename() {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest("clauseo-protocol", ""),
        ),
        ("crates/odd".to_owned(), manifest("odd", "")),
    ];
    let found = run_on(&workspace("odd-name", PROTOCOL_ROW, "", &crates));
    assert_eq!(found.inspected, 1);
    assert_eq!(found.violations.len(), 1);
    assert!(
        found.violations[0]
            .fix
            .starts_with("rename the package to `clauseo-`")
    );
}

#[test]
fn and_lists_read_as_english() {
    let list =
        |items: &[&str]| and_list(&items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
    assert_eq!(list(&["a"]), "a");
    assert_eq!(list(&["a", "b"]), "a and b");
    assert_eq!(list(&["a", "b", "c"]), "a, b and c");
}

#[test]
fn an_entry_whose_line_cannot_be_found_says_so() {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
            ),
        ),
        tauri(),
    ];
    let dir = workspace("unlocated", PROTOCOL_ROW, "", &crates);
    let tree = checks::graph::load_tree(&dir, "clauseo-protocol", &["--offline"])
        .expect("cargo tree runs");
    let table = crates_table::load(&dir).expect("the table parses");
    let row = table.row("protocol").expect("the row is there");
    let found = graph::violations(
        &graph::Crate {
            package: "clauseo-protocol",
            manifest: "crates/protocol/Cargo.toml",
            text: "",
            row,
            deps: &crate::workspace::PackageDependencies::default(),
        },
        &tree,
    );
    let printed: Vec<(u32, String)> = found.iter().map(|v| (v.line, v.message.clone())).collect();
    assert_eq!(
        printed,
        [(
            1,
            "clauseo-protocol depends on tauri, which ARCHITECTURE.md forbids (xtask found no line declaring `tauri` in crates/protocol/Cargo.toml, so this points at line 1)".to_owned()
        )]
    );
}

/// A workspace with protocol and desktop, both using a vendored `spec` whose optional `tauri`
/// sits behind its `tauri` feature; `protocol_spec` and `desktop_spec` are their `spec` lines.
fn spec_feature(name: &str, protocol_spec: &str, desktop_spec: &str) -> Inspection {
    let rows = format!("{PROTOCOL_ROW}| `desktop` | shell | protocol, tauri | | exists |\n");
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                &format!("[dependencies]\n{protocol_spec}\n"),
            ),
        ),
        (
            "crates/desktop".to_owned(),
            manifest(
                "clauseo-desktop",
                &format!(
                    "[dependencies]\n{desktop_spec}\nclauseo-protocol = {{ path = \"../protocol\" }}\n"
                ),
            ),
        ),
        vendor(
            "spec",
            "[dependencies]\ntauri = { path = \"../tauri\", optional = true }\n\n[features]\ntauri = [\"dep:tauri\"]\n",
        ),
        tauri(),
    ];
    run_on(&workspace(name, &rows, "", &crates))
}

pub(super) fn feature_turned_on_by_another_crate_passes() -> Inspection {
    spec_feature(
        "spec-desktop",
        "spec = { path = \"../../vendor/spec\" }",
        "spec = { path = \"../../vendor/spec\", features = [\"tauri\"] }",
    )
}

pub(super) fn feature_turned_on_by_the_crate_itself() -> Inspection {
    spec_feature(
        "spec-protocol",
        "spec = { path = \"../../vendor/spec\", features = [\"tauri\"] }",
        "spec = { path = \"../../vendor/spec\" }",
    )
}
