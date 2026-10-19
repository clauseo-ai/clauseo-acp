//! Planted cases of the `dependencies` check about Never entries that look like our crates: the
//! `(crates.io)` marker, letter case, swapped letters and `clauseo-` names that are no package.

use super::{manifest, run_on, vendor, workspace};
use crate::checks::Inspection;

/// The rows `host` (Never cell `never`), `agents`, `mcp` and `xtask`, host on line 7.
fn rows(never: &str) -> String {
    format!(
        "| `host` | sessions | protocol | tauri, {never} | exists |\n| `agents` | registry | protocol | tauri | planned |\n| `mcp` | sidecar | rmcp | tauri | planned |\n| `xtask` | checks | none of ours | tauri | planned |\n"
    )
}

/// A workspace whose only member is `clauseo-host`, with `deps` as its `[dependencies]` body and
/// a vendored crate per name in `vendored`, under `rows(never)`.
fn host(name: &str, never: &str, deps: &str, vendored: &[&str]) -> Inspection {
    let mut crates = vec![(
        "crates/host".to_owned(),
        manifest("clauseo-host", &format!("[dependencies]\n{deps}")),
    )];
    crates.extend(vendored.iter().map(|package| vendor(package, "")));
    run_on(&workspace(name, &rows(never), "", &crates))
}

pub(in crate::checks::dependencies) fn crates_io_marker_passes() -> Inspection {
    host("marker-passes", "rmcp (crates.io)", "", &[])
}

pub(in crate::checks::dependencies) fn crates_io_marker_bans_the_package() -> Inspection {
    host(
        "marker-bans",
        "rmcp (crates.io)",
        "rmcp = { path = \"../../vendor/rmcp\" }\n",
        &["rmcp"],
    )
}

pub(in crate::checks::dependencies) fn crates_io_name_one_edit_from_a_row() -> Inspection {
    host("rmcp-bare", "rmcp", "", &[])
}

pub(in crate::checks::dependencies) fn row_name_in_capitals() -> Inspection {
    host("capitals", "AGENTS", "", &[])
}

pub(in crate::checks::dependencies) fn package_name_in_other_case() -> Inspection {
    host("clauseo-case", "Clauseo-agents", "", &[])
}

pub(in crate::checks::dependencies) fn row_name_with_two_letters_swapped() -> Inspection {
    host("swapped", "agnets", "", &[])
}

pub(in crate::checks::dependencies) fn clauseo_before_a_row_whose_package_has_no_prefix()
-> Inspection {
    host("clauseo-xtask", "clauseo-xtask", "", &[])
}
