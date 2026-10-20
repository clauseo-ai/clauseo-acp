//! A package reached through two direct dependencies. The `cargo tree` reading itself is tested
//! in `checks/graph/tests.rs`.

use super::{PROTOCOL_ROW, manifest, run_on, tauri, vendor, workspace};
use crate::checks::Inspection;

/// Two direct dependencies that both bring tauri in, through the same `inner`: cargo prints the
/// second path as `inner (*)`, which must still lead to tauri.
pub(in crate::checks::dependencies) fn two_paths_each_fail() -> Inspection {
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
    run_on(&workspace("two-paths", PROTOCOL_ROW, "", &crates))
}

/// `s` declared in protocol's `[dependencies]` with `f1` and in its `[build-dependencies]` with
/// `f2`, where `s`'s feature `f1` (or `f2`) brings `b` in: cargo builds `s` once per feature set,
/// so each declaration has its own node.
fn declared_twice(name: &str, s_features: &str) -> Inspection {
    declared_twice_under("[build-dependencies]", "f2", name, s_features)
}

/// `s` declared in protocol's `[dependencies]` with `f1` and again under the table `header`
/// with `feature`, with the features `s_features` of `s` deciding what brings `b` in.
fn declared_twice_under(header: &str, feature: &str, name: &str, s_features: &str) -> Inspection {
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
    run_on(&workspace(name, rows, "", &crates))
}

/// Only the build declaration's `s` brings `b` in, so the violation points at the build line,
/// line 10, not at the normal line, line 7.
pub(in crate::checks::dependencies) fn declared_twice_blames_the_building_line() -> Inspection {
    declared_twice("declared-twice-build", "f1 = []\nf2 = [\"dep:b\"]")
}

/// Both declarations' `s` bring `b` in, so the violation names both lines.
pub(in crate::checks::dependencies) fn declared_twice_names_both_lines() -> Inspection {
    declared_twice("declared-twice-both", "f1 = [\"dep:b\"]\nf2 = [\"dep:b\"]")
}

/// Only the Windows declaration's `f3` brings `b` in, but both lines are normal dependencies, so
/// `cargo tree --target all` builds them as one node with `f1` and `f3`.
pub(in crate::checks::dependencies) fn declared_twice_in_one_section() -> Inspection {
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
pub(in crate::checks::dependencies) fn host_build_feature_blames_only_its_path() -> Inspection {
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
    run_on(&workspace("host-feature", rows, "", &crates))
}
