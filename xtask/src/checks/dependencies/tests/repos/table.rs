//! The builders of the dependencies cases about the Crates table itself: where the table
//! ends, which Never entries name a crate, and whether a row's Status is true.

use super::*;

pub(in crate::checks::dependencies::tests) fn std_module_in_never() -> Repo {
    let rows = "| `protocol` | events | serde | net, tauri | exists |\n";
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    run_on(workspace("std-module", rows, "", &crates))
}

/// A workspace whose only member is `clauseo-host`, depending on a vendored `clauseo-agents`
/// (so no row is needed for it), under the Crates table `rows`.
fn host_using_agents(name: &str, rows: &str) -> Repo {
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
    run_on(workspace(name, rows, "", &crates))
}

pub(in crate::checks::dependencies::tests) fn row_without_leading_pipe() -> Repo {
    host_using_agents(
        "no-leading-pipe",
        "| `agents` | registry | protocol | tauri | planned |\n`host` | sessions | protocol | tauri, agents | exists |\n",
    )
}

pub(in crate::checks::dependencies::tests) fn prose_with_a_pipe_after_the_table_passes() -> Repo {
    let rows = format!("{PROTOCOL_ROW}\nUse `a | b` in shell.\n");
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    run_on(workspace("prose-pipe", &rows, "", &crates))
}

pub(in crate::checks::dependencies::tests) fn misspelt_row_in_never() -> Repo {
    host_using_agents(
        "misspelt-row",
        "| `host` | sessions | protocol | tauri, agent | exists |\n| `agents` | registry | protocol | tauri | planned |\n",
    )
}

pub(in crate::checks::dependencies::tests) fn clauseo_prefix_names_a_row() -> Repo {
    host_using_agents(
        "clauseo-prefix",
        "| `host` | sessions | protocol | tauri, clauseo-agents | exists |\n| `agents` | registry | protocol | tauri | planned |\n",
    )
}

pub(in crate::checks::dependencies::tests) fn clauseo_prefixed_row_passes() -> Repo {
    host_using_agents(
        "clauseo-process",
        "| `host` | sessions | protocol | tauri, clauseo-process | exists |\n| `process` | spawning | protocol | tauri | planned |\n",
    )
}

pub(in crate::checks::dependencies::tests) fn clauseo_prefix_without_row() -> Repo {
    host_using_agents(
        "clauseo-no-row",
        "| `host` | sessions | protocol | tauri, clauseo-nosuchrow | exists |\n",
    )
}

pub(in crate::checks::dependencies::tests) fn underscore_in_crates_io_name() -> Repo {
    host_using_agents(
        "underscore",
        "| `host` | sessions | protocol | tauri, serde_json | exists |\n",
    )
}

pub(in crate::checks::dependencies::tests) fn exists_row_without_crate() -> Repo {
    let rows = format!("{PROTOCOL_ROW}| `store` | log | protocol | tauri | exists |\n");
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    run_on(workspace("exists-no-crate", &rows, "", &crates))
}

pub(in crate::checks::dependencies::tests) fn planned_row_with_crate() -> Repo {
    let rows = "| `protocol` | events | serde | tauri, tokio, fs | planned |\n";
    let crates = [(
        "crates/protocol".to_owned(),
        manifest("clauseo-protocol", ""),
    )];
    run_on(workspace("planned-with-crate", rows, "", &crates))
}
