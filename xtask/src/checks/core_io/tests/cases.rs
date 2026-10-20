//! The core-io planted cases about files beside the crate's `clippy.toml`: other clippy configs,
//! the manifests' lint levels and the cargo config's rustflags, aliases and `[env]`.

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

/// A clock-only crate's own `[lints.clippy]` would turn the root's clock bans off.
pub(in crate::checks::core_io) fn clock_only_crate_lints_of_its_own() -> Inspection {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "clock");
    run_repo(
        "clock-only-lints-own",
        &Repo {
            architecture: Some(&architecture),
            files: &[(
                "crates/protocol/Cargo.toml",
                "[package]\nname = \"clauseo-protocol\"\n\n[lints.clippy]\n\
                 disallowed_methods = \"allow\"\n",
            )],
            ..Repo::default()
        },
    )
}

/// `clauseo-extra` has no row; it still inherits the workspace lints.
pub(in crate::checks::core_io) fn crate_without_a_row_lints_not_inherited() -> Inspection {
    good_crate_and(
        "row-less-lints-missing",
        &[(
            "crates/extra/Cargo.toml",
            "[package]\nname = \"clauseo-extra\"\n",
        )],
    )
}

/// The review's case: a legacy `.cargo/config` that silences a ban beside a clean
/// `.cargo/config.toml`.
pub(in crate::checks::core_io) fn legacy_cargo_config() -> Inspection {
    good_crate_and(
        "legacy-cargo-config",
        &[
            (
                ".cargo/config",
                "[build]\nrustflags = [\"-Aclippy::disallowed_methods\"]\n",
            ),
            (".cargo/config.toml", "[alias]\nx = \"run\"\n"),
        ],
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

/// The review's case: flags that silence the bans without naming them.
pub(in crate::checks::core_io) fn rustflags_that_cap_lints() -> Inspection {
    good_crate_and(
        "rustflags-cap-lints",
        &[(
            ".cargo/config.toml",
            "[build]\nrustflags = [\"--cap-lints\", \"allow\", \"-A\", \"clippy::style\"]\n",
        )],
    )
}

/// The review's case: `CLIPPY_CONF_DIR` set for clippy-driver through `[env]`, and another
/// `CLIPPY_` entry in table form.
pub(in crate::checks::core_io) fn clippy_env_in_cargo_config() -> Inspection {
    good_crate_and(
        "clippy-env",
        &[(
            ".cargo/config.toml",
            "[env]\nRUST_LOG = \"info\"\nCLIPPY_CONF_DIR = \"/\"\n\
             CLIPPY_ARGS = { value = \"x\", force = true }\n",
        )],
    )
}

/// The reviewer's case: `CLIPPY_CONF_DIR` in mixed case, which Windows reads as the same name.
pub(in crate::checks::core_io) fn clippy_env_in_another_case() -> Inspection {
    good_crate_and(
        "clippy-env-case",
        &[(".cargo/config.toml", "[env]\nClippy_Conf_Dir = \"/\"\n")],
    )
}

/// The review's case: an alias that turns clippy into `cargo check`, beside the repo's own
/// `xtask` alias.
pub(in crate::checks::core_io) fn alias_of_a_command_xtask_runs() -> Inspection {
    good_crate_and(
        "alias-clippy",
        &[(
            ".cargo/config.toml",
            "[alias]\nxtask = \"run --quiet --locked --package xtask --\"\nclippy = \"check\"\n",
        )],
    )
}

/// A cargo config with aliases, an `[env]` entry and a build setting passes. cargo ignores an
/// alias named after a built-in command, such as `check`.
pub(in crate::checks::core_io) fn cargo_config_without_lint_settings() -> Inspection {
    good_crate_and(
        "cargo-config-clean",
        &[(
            ".cargo/config.toml",
            "[alias]\nx = \"run\"\ncheck = \"build\"\n\n[env]\nRUST_LOG = \"info\"\n\n[build]\njobs = 4\n",
        )],
    )
}
