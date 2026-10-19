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
