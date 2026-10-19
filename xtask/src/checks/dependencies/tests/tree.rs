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
