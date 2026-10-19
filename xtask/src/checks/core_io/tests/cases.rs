//! The core-io planted cases about files beside the crate's `clippy.toml`: other clippy configs,
//! the manifests' lint levels and the cargo config's rustflags.

use super::{ARCHITECTURE, Repo, expected, run_repo};
use crate::checks::Inspection;

/// Runs the check with the generated `crates/protocol/clippy.toml` and `files`.
fn good_crate_and(case: &str, files: &[(&str, &str)]) -> Inspection {
    let text = expected(case).text;
    run_repo(
        case,
        &Repo {
            crate_file: Some(&text),
            files,
            ..Repo::default()
        },
    )
}

/// The review's case: a `.clippy.toml` beside a correct `clippy.toml`, which clippy reads
/// instead.
pub(in crate::checks::core_io) fn dot_clippy_toml_beside_the_crate_file() -> Inspection {
    good_crate_and(
        "dot-clippy-toml-beside",
        &[(
            "crates/protocol/.clippy.toml",
            "too-many-lines-threshold = 120\n",
        )],
    )
}

pub(in crate::checks::core_io) fn dot_clippy_toml_at_the_root() -> Inspection {
    good_crate_and(
        "dot-clippy-toml-root",
        &[(".clippy.toml", "msrv = \"1.99\"\n")],
    )
}

pub(in crate::checks::core_io) fn clippy_toml_in_crates() -> Inspection {
    good_crate_and("clippy-toml-in-crates", &[("crates/clippy.toml", "")])
}

pub(in crate::checks::core_io) fn clippy_toml_in_xtask() -> Inspection {
    good_crate_and("clippy-toml-in-xtask", &[("xtask/clippy.toml", "")])
}

/// `clauseo-extra` has no row, so its own file would hide the root bans from it.
pub(in crate::checks::core_io) fn clippy_toml_of_a_crate_without_a_row() -> Inspection {
    good_crate_and(
        "clippy-toml-without-row",
        &[("crates/extra/clippy.toml", "msrv = \"1.99\"\n")],
    )
}

/// A row with `clock` alone gets the root file; a file of its own would hide the clock bans.
pub(in crate::checks::core_io) fn clippy_toml_of_a_clock_only_crate() -> Inspection {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "clock");
    run_repo(
        "clippy-toml-clock-only",
        &Repo {
            crate_file: Some("too-many-lines-threshold = 120\n"),
            architecture: Some(&architecture),
            ..Repo::default()
        },
    )
}

pub(in crate::checks::core_io) fn crate_lints_not_inherited() -> Inspection {
    good_crate_and(
        "crate-lints-missing",
        &[(
            "crates/protocol/Cargo.toml",
            "[package]\nname = \"clauseo-protocol\"\n",
        )],
    )
}

/// The review's case: the crate's own `[lints.clippy]` turns a ban off.
pub(in crate::checks::core_io) fn crate_lints_of_its_own() -> Inspection {
    good_crate_and(
        "crate-lints-own",
        &[(
            "crates/protocol/Cargo.toml",
            "[package]\nname = \"clauseo-protocol\"\n\n[lints.clippy]\n\
             disallowed_methods = \"allow\"\n",
        )],
    )
}

pub(in crate::checks::core_io) fn workspace_lint_allowed() -> Inspection {
    good_crate_and(
        "workspace-lint-allowed",
        &[(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.lints.clippy]\n\
             pedantic = { level = \"warn\", priority = -1 }\n\
             disallowed_types = { level = \"allow\", priority = 1 }\n\
             disallowed-methods = \"expect\"\n",
        )],
    )
}

/// Each level that reports, in each spelling cargo takes.
pub(in crate::checks::core_io) fn workspace_lints_that_report() -> Inspection {
    good_crate_and(
        "workspace-lints-report",
        &[(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.lints.clippy]\n\
             disallowed-methods = \"warn\"\ndisallowed_types = { level = \"forbid\" }\n",
        )],
    )
}

pub(in crate::checks::core_io) fn rustflags_in_build() -> Inspection {
    good_crate_and(
        "rustflags-build",
        &[(
            ".cargo/config.toml",
            "[alias]\nx = \"run\"\n\n[build]\nrustflags = [\"-A\", \"clippy::disallowed_methods\"]\n",
        )],
    )
}

pub(in crate::checks::core_io) fn rustflags_in_a_target() -> Inspection {
    good_crate_and(
        "rustflags-target",
        &[(
            ".cargo/config.toml",
            "[target.'cfg(unix)']\nrustflags = \"-Dwarnings -Aclippy::disallowed-types\"\n",
        )],
    )
}

/// Rustflags that name no ban pass.
pub(in crate::checks::core_io) fn rustflags_without_a_ban() -> Inspection {
    good_crate_and(
        "rustflags-other",
        &[(
            ".cargo/config.toml",
            "[build]\nrustflags = [\"-Ctarget-cpu=native\"]\n\n\
             [target.x86_64-unknown-linux-gnu]\nrustflags = \"-Clink-arg=-fuse-ld=lld\"\n",
        )],
    )
}
