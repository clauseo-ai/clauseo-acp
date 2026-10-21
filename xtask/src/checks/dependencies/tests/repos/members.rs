//! The builders of the dependencies cases about workspace members: a member with no row, a
//! member that depends on another, and a package spelled with `-`.

use super::*;

pub(in crate::checks::dependencies::tests) fn no_row() -> Repo {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest("clauseo-protocol", ""),
        ),
        ("crates/store".to_owned(), manifest("clauseo-store", "")),
    ];
    run_on(workspace("no-row", PROTOCOL_ROW, "", &crates))
}

pub(in crate::checks::dependencies::tests) fn forbidden_workspace_crate() -> Repo {
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
    run_on(workspace("workspace-crate", rows, "", &crates))
}

pub(in crate::checks::dependencies::tests) fn dash_package_without_row() -> Repo {
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
    run_on(workspace("dash-no-row", PROTOCOL_ROW, "", &crates))
}

pub(in crate::checks::dependencies::tests) fn workspace_crate_spelled_with_dash() -> Repo {
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
    run_on(workspace("dash-workspace", rows, "", &crates))
}

pub(in crate::checks::dependencies::tests) fn planned_row_without_crate_is_skipped() -> Repo {
    let rows = format!("{PROTOCOL_ROW}| `host` | sessions | protocol | tauri | planned |\n");
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    run_on(workspace("planned", &rows, "", &crates))
}
