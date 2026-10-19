//! Planted cases of the `dependencies` check about the Crates table itself: where the table
//! ends, which Never entries name a crate, and whether a row's Status is true.

use super::{PROTOCOL_ROW, manifest, run_on, vendor, workspace};
use crate::checks::Inspection;

/// A workspace whose only member is `clauseo-host`, depending on a vendored `clauseo-agents`
/// (so no row is needed for it), under the Crates table `rows`.
fn host_using_agents(name: &str, rows: &str) -> Inspection {
    let crates = [
        (
            "crates/host".to_owned(),
            manifest(
                "clauseo-host",
                "[dependencies]\nclauseo-agents = { path = \"../../vendor/clauseo-agents\" }\n",
            ),
        ),
        vendor("clauseo-agents", ""),
    ];
    run_on(&workspace(name, rows, "", &crates))
}

pub(in crate::checks::dependencies) fn row_without_leading_pipe() -> Inspection {
    host_using_agents(
        "no-leading-pipe",
        "| `host` | sessions | protocol | tauri, agents | exists |\n`legal` | data | protocol | tauri | planned |\n| `agents` | registry | protocol | tauri | planned |\n",
    )
}

pub(in crate::checks::dependencies) fn misspelt_row_in_never() -> Inspection {
    host_using_agents(
        "misspelt-row",
        "| `host` | sessions | protocol | tauri, agent | exists |\n| `agents` | registry | protocol | tauri | planned |\n",
    )
}

pub(in crate::checks::dependencies) fn clauseo_prefix_names_a_row() -> Inspection {
    host_using_agents(
        "clauseo-prefix",
        "| `host` | sessions | protocol | tauri, clauseo-agents | exists |\n| `agents` | registry | protocol | tauri | planned |\n",
    )
}

pub(in crate::checks::dependencies) fn clauseo_prefixed_row_passes() -> Inspection {
    host_using_agents(
        "clauseo-process",
        "| `host` | sessions | protocol | tauri, clauseo-process | exists |\n| `process` | spawning | protocol | tauri | planned |\n",
    )
}

pub(in crate::checks::dependencies) fn clauseo_prefix_without_row() -> Inspection {
    host_using_agents(
        "clauseo-no-row",
        "| `host` | sessions | protocol | tauri, clauseo-nosuchrow | exists |\n",
    )
}

pub(in crate::checks::dependencies) fn underscore_in_crates_io_name() -> Inspection {
    host_using_agents(
        "underscore",
        "| `host` | sessions | protocol | tauri, serde_json | exists |\n",
    )
}

pub(in crate::checks::dependencies) fn exists_row_without_crate() -> Inspection {
    let rows = format!("{PROTOCOL_ROW}| `store` | log | protocol | tauri | exists |\n");
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    run_on(&workspace("exists-no-crate", &rows, "", &crates))
}

pub(in crate::checks::dependencies) fn planned_row_with_crate() -> Inspection {
    let rows = "| `protocol` | events | serde | tauri, tokio, fs | planned |\n";
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    run_on(&workspace("planned-with-crate", rows, "", &crates))
}
