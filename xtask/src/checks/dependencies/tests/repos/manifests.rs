//! The builders of the dependencies cases that declare tauri (or another crate) in protocol's
//! manifest: each table, key, target and feature form of the declaration.

use super::*;

pub(in crate::checks::dependencies::tests) fn direct() -> Repo {
    protocol(
        "direct",
        "[dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(in crate::checks::dependencies::tests) fn transitive() -> Repo {
    protocol(
        "transitive",
        "[dependencies]\nmid = { path = \"../../vendor/mid\" }\n",
        vec![
            vendor("mid", "[dependencies]\ninner = { path = \"../inner\" }\n"),
            vendor("inner", "[dependencies]\ntauri = { path = \"../tauri\" }\n"),
            tauri(),
        ],
    )
}

pub(in crate::checks::dependencies::tests) fn build_dependency() -> Repo {
    protocol(
        "build",
        "[build-dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(in crate::checks::dependencies::tests) fn renamed() -> Repo {
    protocol(
        "renamed",
        "[dependencies]\nt = { package = \"tauri\", path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(in crate::checks::dependencies::tests) fn target_windows() -> Repo {
    protocol(
        "windows",
        "[target.'cfg(windows)'.dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(in crate::checks::dependencies::tests) fn inherited() -> Repo {
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\ntauri.workspace = true\n",
            ),
        ),
        tauri(),
    ];
    let dir = workspace(
        "inherited",
        PROTOCOL_ROW,
        "tauri = { path = \"vendor/tauri\" }\n",
        &crates,
    );
    run_on(dir)
}

pub(in crate::checks::dependencies::tests) fn optional_feature() -> Repo {
    protocol(
        "optional",
        "[dependencies]\ntauri = { path = \"../../vendor/tauri\", optional = true }\n\n[features]\nwindow = [\"dep:tauri\"]\n",
        vec![tauri()],
    )
}

pub(in crate::checks::dependencies::tests) fn comment_after_header() -> Repo {
    protocol(
        "comment",
        "[dependencies] # runtime\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(in crate::checks::dependencies::tests) fn quoted_key() -> Repo {
    protocol(
        "quoted",
        "[dependencies]\n'tauri' = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(in crate::checks::dependencies::tests) fn table_header() -> Repo {
    protocol(
        "table-header",
        "[dependencies.tauri]\npath = \"../../vendor/tauri\"\n",
        vec![tauri()],
    )
}

pub(in crate::checks::dependencies::tests) fn crates_io_spelled_with_underscore() -> Repo {
    let rows = "| `protocol` | events | serde | async-process | exists |\n";
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                "[dependencies]\nasync_process = { path = \"../../vendor/async_process\" }\n",
            ),
        ),
        vendor("async_process", ""),
    ];
    run_on(workspace("dash-crates-io", rows, "", &crates))
}

pub(in crate::checks::dependencies::tests) fn dev_dependency_passes() -> Repo {
    protocol(
        "dev",
        "[dev-dependencies]\ntauri = { path = \"../../vendor/tauri\" }\n",
        vec![tauri()],
    )
}

pub(in crate::checks::dependencies::tests) fn clean_workspace_passes() -> Repo {
    protocol(
        "clean",
        "[dependencies]\nserde = { path = \"../../vendor/serde\" }\n",
        vec![vendor("serde", "")],
    )
}

/// A workspace with protocol and desktop, both using a vendored `spec` whose optional `tauri`
/// sits behind its `tauri` feature; `protocol_spec` and `desktop_spec` are their `spec` lines.
fn spec_feature(name: &str, protocol_spec: &str, desktop_spec: &str) -> Repo {
    let rows = format!("{PROTOCOL_ROW}| `desktop` | shell | protocol, tauri | | exists |\n");
    let crates = [
        (
            "crates/protocol".to_owned(),
            manifest(
                "clauseo-protocol",
                &format!("[dependencies]\n{protocol_spec}\n"),
            ),
        ),
        (
            "crates/desktop".to_owned(),
            manifest(
                "clauseo-desktop",
                &format!(
                    "[dependencies]\n{desktop_spec}\nclauseo-protocol = {{ path = \"../protocol\" }}\n"
                ),
            ),
        ),
        vendor(
            "spec",
            "[dependencies]\ntauri = { path = \"../tauri\", optional = true }\n\n[features]\ntauri = [\"dep:tauri\"]\n",
        ),
        tauri(),
    ];
    run_on(workspace(name, &rows, "", &crates))
}

pub(in crate::checks::dependencies::tests) fn feature_turned_on_by_another_crate_passes() -> Repo {
    spec_feature(
        "spec-desktop",
        "spec = { path = \"../../vendor/spec\" }",
        "spec = { path = \"../../vendor/spec\", features = [\"tauri\"] }",
    )
}

pub(in crate::checks::dependencies::tests) fn feature_turned_on_by_the_crate_itself() -> Repo {
    spec_feature(
        "spec-protocol",
        "spec = { path = \"../../vendor/spec\", features = [\"tauri\"] }",
        "spec = { path = \"../../vendor/spec\" }",
    )
}
