//! The builders of the ways around the graph walk that the other cases do not take: a forbidden
//! crate reached through another workspace member, and a registry dependency that
//! `[patch.crates-io]` swaps for a path crate that brings the forbidden crate in.
//! (`[replace]` needs the registry index to resolve the version it replaces, so no offline
//! case can plant it.)

use super::*;

/// protocol depends on the member `clauseo-util`, whose row bans nothing, and util depends on
/// tauri: the walk follows members too.
pub(in crate::checks::dependencies::tests) fn through_another_member() -> Repo {
    let rows = format!("{PROTOCOL_ROW}| `util` | helpers | serde | | exists |\n");
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\nclauseo-util = { path = \"../util\" }\n",
            ),
        ),
        (
            "crates/util".to_owned(),
            manifest(
                "clauseo-util",
                "[dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
            ),
        ),
        tauri(),
    ];
    run_on(workspace("through-member", &rows, "", &crates))
}

/// protocol depends on the registry `serde = "1"`, and the root manifest's `[patch.crates-io]`
/// (after an empty `[workspace.dependencies]`) swaps it for `vendor/serde` 1.0.0, which depends
/// on tauri. The version matches the requirement, else cargo would leave the patch unused.
pub(in crate::checks::dependencies::tests) fn patched_registry_dependency() -> Repo {
    let patch = "\n[patch.crates-io]\nserde = { path = \"vendor/serde\" }\n";
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest("clauseo-protocol", "[dependencies]\nserde = \"1\"\n"),
        ),
        (
            "vendor/serde".to_owned(),
            manifest("serde", "[dependencies]\ntauri = { path = \"../tauri\" }\n")
                .replace("0.1.0", "1.0.0"),
        ),
        tauri(),
    ];
    run_on(workspace("patched", PROTOCOL_ROW, patch, &crates))
}
