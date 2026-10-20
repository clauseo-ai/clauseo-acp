//! A package reached through two direct dependencies. The `cargo tree` reading itself is tested
//! in `checks/graph/tests.rs`.

use super::{PROTOCOL_ROW, manifest, run_on, tauri, vendor, workspace};
use crate::checks::dependencies::Fix;
use crate::checks::planted::Repo;
use crate::checks::violation;

/// Two direct dependencies that both bring tauri in, through the same `inner`: cargo prints the
/// second path as `inner (*)`, which must still lead to tauri.
pub(in crate::checks::dependencies) fn two_paths_each_fail() -> Repo {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\nleft = { path = \"../../vendor/left\" }\nright = { path = \"../../vendor/right\" }\n",
            ),
        ),
        vendor("left", "[dependencies]\ninner = { path = \"../inner\" }\n"),
        vendor("right", "[dependencies]\ninner = { path = \"../inner\" }\n"),
        vendor("inner", "[dependencies]\ntauri = { path = \"../tauri\" }\n"),
        tauri(),
    ];
    run_on(workspace("two-paths", PROTOCOL_ROW, "", &crates))
}

/// `s` declared in protocol's `[dependencies]` with `f1` and in its `[build-dependencies]` with
/// `f2`, where `s`'s feature `f1` (or `f2`) brings `b` in: cargo builds `s` once per feature set,
/// so each declaration has its own node.
fn declared_twice(name: &str, s_features: &str) -> Repo {
    declared_twice_under("[build-dependencies]", "f2", name, s_features)
}

/// `s` declared in protocol's `[dependencies]` with `f1` and again under the table `header`
/// with `feature`, with the features `s_features` of `s` deciding what brings `b` in.
fn declared_twice_under(header: &str, feature: &str, name: &str, s_features: &str) -> Repo {
    let deps = format!(
        "[dependencies]\ns = {{ path = \"../../vendor/s\", features = [\"f1\"] }}\n\n{header}\ns = {{ path = \"../../vendor/s\", features = [\"{feature}\"] }}\n"
    );
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest("clauseo-protocol", &deps),
        ),
        vendor(
            "s",
            &format!(
                "[dependencies]\nb = {{ path = \"../b\", optional = true }}\n\n[features]\n{s_features}\n"
            ),
        ),
        vendor("b", ""),
    ];
    let rows = "| `protocol` | events | serde | b | exists |\n";
    run_on(workspace(name, rows, "", &crates))
}

/// Only the build declaration's `s` brings `b` in, so the violation points at the build line,
/// line 10, not at the normal line, line 7.
pub(in crate::checks::dependencies) fn declared_twice_blames_the_building_line() -> Repo {
    declared_twice("declared-twice-build", "f1 = []\nf2 = [\"dep:b\"]")
}

/// Both declarations' `s` bring `b` in, so the violation names both lines.
pub(in crate::checks::dependencies) fn declared_twice_names_both_lines() -> Repo {
    declared_twice("declared-twice-both", "f1 = [\"dep:b\"]\nf2 = [\"dep:b\"]")
}

/// Only the Windows declaration's `f3` brings `b` in, but both lines are normal dependencies, so
/// `cargo tree --target all` builds them as one node with `f1` and `f3`.
pub(in crate::checks::dependencies) fn declared_twice_in_one_section() -> Repo {
    declared_twice_under(
        "[target.'cfg(windows)'.dependencies]",
        "f3",
        "declared-twice-one-section",
        "f1 = []\nf3 = [\"dep:b\"]",
    )
}

/// `x` and the build dependency `y` both use `foo`, and only `y` turns on its feature `f`, which
/// pulls in `b`: cargo builds `foo` twice (for the target without `f`, for the host with it), so
/// `b` is reached through `y` only.
pub(in crate::checks::dependencies) fn host_build_feature_blames_only_its_path() -> Repo {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\nx = { path = \"../../vendor/x\" }\n\n[build-dependencies]\ny = { path = \"../../vendor/y\" }\n",
            ),
        ),
        vendor("x", "[dependencies]\nfoo = { path = \"../foo\" }\n"),
        vendor(
            "y",
            "[dependencies]\nfoo = { path = \"../foo\", features = [\"f\"] }\n",
        ),
        vendor(
            "foo",
            "[dependencies]\nb = { path = \"../b\", optional = true }\n\n[features]\nf = [\"dep:b\"]\n",
        ),
        vendor("b", ""),
    ];
    let rows = "| `protocol` | events | serde | b | exists |\n";
    run_on(workspace("host-feature", rows, "", &crates))
}

/// `s1 = { package = "s", path = "../../vendor/s1" }` in `[dependencies]` and `s2`, the same
/// for `vendor/s2`, in a `[target.'cfg(windows)'.dependencies]` table. They are two packages
/// named `s`, 1.0.0 and 2.0.0, with the bodies `s1_body` and `s2_body`.
fn two_paths_of_one_name(name: &str, s1_body: &str, s2_body: &str) -> Repo {
    let deps = "[dependencies]\ns1 = { package = \"s\", path = \"../../vendor/s1\" }\n\n[target.'cfg(windows)'.dependencies]\ns2 = { package = \"s\", path = \"../../vendor/s2\" }\n";
    let s = |version: &str, body: &str| manifest("s", body).replace("0.1.0", version);
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest("clauseo-protocol", deps),
        ),
        ("vendor/s1".to_owned(), s("1.0.0", s1_body)),
        ("vendor/s2".to_owned(), s("2.0.0", s2_body)),
        vendor("b", ""),
    ];
    let rows = "| `protocol` | events | serde | b | exists |\n";
    run_on(workspace(name, rows, "", &crates))
}

/// The body of a vendored crate that depends on `b`.
const BRINGS_B: &str = "[dependencies]\nb = { path = \"../b\" }\n";

/// Only `s` 2.0.0, declared as `s2` at line 10, brings `b` in, so only line 10 is blamed.
pub(in crate::checks::dependencies) fn two_paths_of_one_name_blame_the_one_that_brings_it_in()
-> Repo {
    two_paths_of_one_name("two-paths-one-name", "", BRINGS_B)
}

/// Both packages named `s` bring `b` in, so both keys and both lines are named.
pub(in crate::checks::dependencies) fn two_paths_of_one_name_both_bring_it_in() -> Repo {
    two_paths_of_one_name("two-paths-one-name-both", BRINGS_B, BRINGS_B)
}

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
