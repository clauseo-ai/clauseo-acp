//! The builders of the dependencies cases about a package reached through two declarations or
//! two paths. The `cargo tree` reading itself is tested in `checks/graph/tests.rs`.

use super::*;

/// Two direct dependencies that both bring tauri in, through the same `inner`: cargo prints the
/// second path as `inner (*)`, which must still lead to tauri.
pub(in crate::checks::dependencies::tests) fn two_paths_each_fail() -> Repo {
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
pub(in crate::checks::dependencies::tests) fn declared_twice_blames_the_building_line() -> Repo {
    declared_twice("declared-twice-build", "f1 = []\nf2 = [\"dep:b\"]")
}

/// Both declarations' `s` bring `b` in, so the violation names both lines.
pub(in crate::checks::dependencies::tests) fn declared_twice_names_both_lines() -> Repo {
    declared_twice("declared-twice-both", "f1 = [\"dep:b\"]\nf2 = [\"dep:b\"]")
}

/// Only the Windows declaration's `f3` brings `b` in, but both lines are normal dependencies, so
/// `cargo tree --target all` builds them as one node with `f1` and `f3`.
pub(in crate::checks::dependencies::tests) fn declared_twice_in_one_section() -> Repo {
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
pub(in crate::checks::dependencies::tests) fn host_build_feature_blames_only_its_path() -> Repo {
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
pub(in crate::checks::dependencies::tests) fn two_paths_of_one_name_blame_the_one_that_brings_it_in()
-> Repo {
    two_paths_of_one_name("two-paths-one-name", "", BRINGS_B)
}

/// Both packages named `s` bring `b` in, so both keys and both lines are named.
pub(in crate::checks::dependencies::tests) fn two_paths_of_one_name_both_bring_it_in() -> Repo {
    two_paths_of_one_name("two-paths-one-name-both", BRINGS_B, BRINGS_B)
}
