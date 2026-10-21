//! Tests for the `dependencies` check that its planted cases cannot express: a lockfile behind the
//! manifests, a check run without the graph, the naming rule and a line that cannot be found. The
//! planted cases are in `planted.rs`, built in `repos.rs`; the fix lines they share are here.

pub(super) mod planted;
mod repos;
mod tree;

use super::*;
use crate::checks::and_list;
use crate::scratch::Scratch;
use crate::workspace;
use repos::*;
use std::fs;

/// The fix that moves `package` out of protocol's manifest, where the one declaration `key`
/// brings it in: protocol's row (line 7) forbids `package` for `clauseo-protocol`.
fn protocol_move(key: &str, package: &str) -> Fix {
    Fix::MoveFromManifest {
        keys: format!("`{key}`"),
        package: package.to_owned(),
        manifest: "crates/protocol/Cargo.toml".to_owned(),
        features: false,
        row: 7,
        krate: "clauseo-protocol".to_owned(),
    }
}

/// The fix that moves `package` out of host's manifest, where the one declaration `key` brings
/// it in: host's row (line 7) forbids `package` for `clauseo-host`.
fn host_move(key: &str, package: &str) -> Fix {
    Fix::MoveFromManifest {
        keys: format!("`{key}`"),
        package: package.to_owned(),
        manifest: "crates/host/Cargo.toml".to_owned(),
        features: false,
        row: 7,
        krate: "clauseo-host".to_owned(),
    }
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
        failure.fix.to_string().contains("cargo update --workspace"),
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
    let found = run(run_on(workspace("odd-name", PROTOCOL_ROW, "", &crates)).ctx());
    assert_eq!(found.inspected, 1);
    assert_eq!(found.violations.len(), 1);
    assert!(
        found.violations[0]
            .fix
            .to_string()
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
