//! Unit tests for how the `dependencies` check reads `cargo tree` and `cargo metadata`: a
//! registry or git package, and the line each declaration sits at. The packages reached through
//! two declarations, as planted cases, are built in `repos/tree.rs`.

use crate::checks::dependencies::Fix;
use crate::checks::violation;

/// The violations of a protocol crate whose manifest is `text`, whose graph is the `cargo tree`
/// output `stdout`, and whose dependencies are the `cargo metadata` list `dependencies`, with a
/// row that bans `b`. Registry and git packages need the network, so these cases write the
/// graph and the metadata out.
fn written_out(text: &str, stdout: &str, dependencies: &str) -> Vec<crate::violation::Violation> {
    use crate::checks::crates_table::{CrateRow, Never, Status};
    use crate::checks::dependencies::graph::{Crate, violations};
    use crate::checks::graph::Tree;
    let tree = Tree::parse(stdout, "clauseo-protocol").expect("the tree parses");
    let deps = crate::workspace::PackageDependencies {
        dependencies: serde_json::from_str(dependencies).expect("the metadata parses"),
        features: std::collections::BTreeMap::new(),
    };
    let row = CrateRow {
        line: 7,
        name: "protocol".to_owned(),
        never: vec![Never::External("b".to_owned())],
        status: Status::Exists,
    };
    let krate = Crate {
        package: "clauseo-protocol",
        manifest: "crates/protocol/Cargo.toml",
        text,
        row: &row,
        deps: &deps,
    };
    violations(&krate, &tree)
}

/// `s1 = { package = "s", version = "1" }` under `cfg(unix)` at line 7 and `s2`, the same
/// with `version = "2"`, under `cfg(windows)` at line 10, where only `s` 2.0.0 brings `b` in.
#[test]
fn a_registry_declaration_is_matched_to_the_version_its_requirement_allows() {
    let text = "[package]\nname = \"clauseo-protocol\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[target.'cfg(unix)'.dependencies]\ns1 = { package = \"s\", version = \"1\" }\n\n[target.'cfg(windows)'.dependencies]\ns2 = { package = \"s\", version = \"2\" }\n";
    let stdout =
        "clauseo-protocol v0.1.0 []\n|-- s v1.4.0 []\n`-- s v2.0.0 []\n    `-- b v1.0.0 []\n";
    let dependencies = r#"[
        {"name": "s", "rename": "s1", "kind": null, "target": "cfg(unix)", "optional": false,
         "features": [], "req": "^1"},
        {"name": "s", "rename": "s2", "kind": null, "target": "cfg(windows)", "optional": false,
         "features": [], "req": "^2"}]"#;
    assert_eq!(
        written_out(text, stdout, dependencies),
        vec![violation(
            "crates/protocol/Cargo.toml",
            10,
            "clauseo-protocol depends on b through s, which ARCHITECTURE.md forbids",
            Fix::MoveFromManifest {
                keys: "`s2`".to_owned(),
                package: "b".to_owned(),
                manifest: "crates/protocol/Cargo.toml".to_owned(),
                features: false,
                row: 7,
                krate: "clauseo-protocol".to_owned(),
            },
        )]
    );
}

/// The violations of `s1 = { package = "s", version = "1" }` at line 7 and `s2 = { package =
/// "s", git = "https://example.com/s", branch = "main" }` under `cfg(windows)` at line 10, with
/// `patch` after them, for the graph `stdout`. The metadata writes each source as `cargo
/// metadata` does, and `stdout` prints the git node as `cargo tree` does.
fn registry_and_git(patch: &str, stdout: &str) -> Vec<crate::violation::Violation> {
    let text = format!(
        "[package]\nname = \"clauseo-protocol\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ns1 = {{ package = \"s\", version = \"1\" }}\n\n[target.'cfg(windows)'.dependencies]\ns2 = {{ package = \"s\", git = \"https://example.com/s\", branch = \"main\" }}\n{patch}"
    );
    let dependencies = r#"[
        {"name": "s", "rename": "s1", "kind": null, "target": null, "optional": false,
         "features": [], "req": "^1",
         "source": "registry+https://github.com/rust-lang/crates.io-index"},
        {"name": "s", "rename": "s2", "kind": null, "target": "cfg(windows)", "optional": false,
         "features": [], "req": "*", "source": "git+https://example.com/s?branch=main"}]"#;
    written_out(&text, stdout, dependencies)
}

/// The registry package `s` 1.4.0 and the git package `s` 1.2.0, which alone brings `b` in.
/// Both versions satisfy `s1`'s requirement, but `s1` is matched to the registry package and
/// `s2` to the git package by its source, so only line 10 is blamed.
#[test]
fn a_git_declaration_next_to_a_registry_declaration_is_matched_by_its_source() {
    let stdout = "clauseo-protocol v0.1.0 []\n|-- s v1.4.0 []\n`-- s v1.2.0 (https://example.com/s?branch=main#0123abcd) []\n    `-- b v1.0.0 []\n";
    assert_eq!(
        registry_and_git("", stdout),
        vec![violation(
            "crates/protocol/Cargo.toml",
            10,
            "clauseo-protocol depends on b through s, which ARCHITECTURE.md forbids",
            Fix::MoveFromManifest {
                keys: "`s2`".to_owned(),
                package: "b".to_owned(),
                manifest: "crates/protocol/Cargo.toml".to_owned(),
                features: false,
                row: 7,
                krate: "clauseo-protocol".to_owned(),
            },
        )]
    );
}

/// `[patch.crates-io]` points `s1` at the path package `vendor/s` 1.0.0, and both it and the git
/// package `s` 1.2.0 bring `b` in. `s1` is matched to the path package, not to the git package,
/// so each line is known to bring `b` in.
#[test]
fn a_registry_declaration_is_not_matched_to_a_git_package_of_its_name() {
    let patch = "\n[patch.crates-io]\ns = { path = \"../../vendor/s\" }\n";
    let stdout = "clauseo-protocol v0.1.0 []\n|-- s v1.0.0 (/ws/vendor/s) []\n|   `-- b v1.0.0 []\n`-- s v1.2.0 (https://example.com/s?branch=main#0123abcd) []\n    `-- b v1.0.0 []\n";
    assert_eq!(
        registry_and_git(patch, stdout),
        vec![violation(
            "crates/protocol/Cargo.toml",
            7,
            "clauseo-protocol depends on b through s, which ARCHITECTURE.md forbids. `s1` and `s2` are declared at lines 7 and 10 of crates/protocol/Cargo.toml, and each declaration brings `b` in",
            Fix::MoveFromLines {
                keys: "`s1` and `s2`".to_owned(),
                package: "b".to_owned(),
                lines: "7 and 10".to_owned(),
                manifest: "crates/protocol/Cargo.toml".to_owned(),
                features: false,
                row: 7,
                krate: "clauseo-protocol".to_owned(),
            },
        )]
    );
}
