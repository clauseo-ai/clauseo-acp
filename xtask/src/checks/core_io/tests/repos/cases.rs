//! The core-io planted cases about files beside the crate's `clippy.toml`: other clippy configs,
//! the manifests' lint levels and the cargo config's rustflags, aliases and `[env]`.

use super::{ARCHITECTURE, Layout, expected, run_repo};
use crate::checks::planted::Repo;

/// The repo with the generated `crates/protocol/clippy.toml` and `files`.
fn good_crate_and(case: &str, files: &[(&str, &str)]) -> Repo {
    let text = expected(case).text;
    run_repo(
        case,
        &Layout {
            crate_file: Some(&text),
            files,
            ..Layout::default()
        },
    )
}

/// The review's case: a `.clippy.toml` beside a correct `clippy.toml`, which clippy reads
/// instead.
pub(in crate::checks::core_io::tests) fn dot_clippy_toml_beside_the_crate_file() -> Repo {
    good_crate_and(
        "dot-clippy-toml-beside",
        &[(
            "crates/protocol/.clippy.toml",
            "too-many-lines-threshold = 120\n",
        )],
    )
}

pub(in crate::checks::core_io::tests) fn dot_clippy_toml_at_the_root() -> Repo {
    good_crate_and(
        "dot-clippy-toml-root",
        &[(".clippy.toml", "msrv = \"1.99\"\n")],
    )
}

pub(in crate::checks::core_io::tests) fn clippy_toml_in_crates() -> Repo {
    good_crate_and("clippy-toml-in-crates", &[("crates/clippy.toml", "")])
}

pub(in crate::checks::core_io::tests) fn clippy_toml_in_xtask() -> Repo {
    good_crate_and("clippy-toml-in-xtask", &[("xtask/clippy.toml", "")])
}

/// `clauseo-extra` has no row, so its own file would hide the root bans from it.
pub(in crate::checks::core_io::tests) fn clippy_toml_of_a_crate_without_a_row() -> Repo {
    good_crate_and(
        "clippy-toml-without-row",
        &[("crates/extra/clippy.toml", "msrv = \"1.99\"\n")],
    )
}

/// A row with `clock` alone gets the root file; a file of its own would hide the clock bans.
pub(in crate::checks::core_io::tests) fn clippy_toml_of_a_clock_only_crate() -> Repo {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "clock");
    run_repo(
        "clippy-toml-clock-only",
        &Layout {
            crate_file: Some("too-many-lines-threshold = 120\n"),
            architecture: Some(&architecture),
            ..Layout::default()
        },
    )
}

pub(in crate::checks::core_io::tests) fn crate_lints_not_inherited() -> Repo {
    good_crate_and(
        "crate-lints-missing",
        &[(
            "crates/protocol/Cargo.toml",
            "[package]\nname = \"clauseo-protocol\"\n",
        )],
    )
}

/// The review's case: the crate's own `[lints.clippy]` turns a ban off.
pub(in crate::checks::core_io::tests) fn crate_lints_of_its_own() -> Repo {
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
pub(in crate::checks::core_io::tests) fn clock_only_crate_lints_of_its_own() -> Repo {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "clock");
    run_repo(
        "clock-only-lints-own",
        &Layout {
            architecture: Some(&architecture),
            files: &[(
                "crates/protocol/Cargo.toml",
                "[package]\nname = \"clauseo-protocol\"\n\n[lints.clippy]\n\
                 disallowed_methods = \"allow\"\n",
            )],
            ..Layout::default()
        },
    )
}

/// `clauseo-extra` has no row; it still inherits the workspace lints.
pub(in crate::checks::core_io::tests) fn crate_without_a_row_lints_not_inherited() -> Repo {
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
pub(in crate::checks::core_io::tests) fn legacy_cargo_config() -> Repo {
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

pub(in crate::checks::core_io::tests) fn rustflags_in_build() -> Repo {
    good_crate_and(
        "rustflags-build",
        &[(
            ".cargo/config.toml",
            "[alias]\nx = \"run\"\n\n[build]\nrustflags = [\"-A\", \"clippy::disallowed_methods\"]\n",
        )],
    )
}

pub(in crate::checks::core_io::tests) fn rustflags_in_a_target() -> Repo {
    good_crate_and(
        "rustflags-target",
        &[(
            ".cargo/config.toml",
            "[target.'cfg(unix)']\nrustflags = \"-Dwarnings -Aclippy::disallowed-types\"\n",
        )],
    )
}

/// The review's case: flags that silence the bans without naming them.
pub(in crate::checks::core_io::tests) fn rustflags_that_cap_lints() -> Repo {
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
pub(in crate::checks::core_io::tests) fn clippy_env_in_cargo_config() -> Repo {
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
pub(in crate::checks::core_io::tests) fn clippy_env_in_another_case() -> Repo {
    good_crate_and(
        "clippy-env-case",
        &[(".cargo/config.toml", "[env]\nClippy_Conf_Dir = \"/\"\n")],
    )
}

/// `rustflags` as a dotted key at the top of the file, outside any table header.
pub(in crate::checks::core_io::tests) fn rustflags_as_a_dotted_key() -> Repo {
    good_crate_and(
        "rustflags-dotted",
        &[(
            ".cargo/config.toml",
            "[alias]\nx = \"run\"\n\n[target]\nx86_64-unknown-linux-gnu.rustflags = [\"-Awarnings\"]\n",
        )],
    )
}

/// `rustflags` inside an inline table.
pub(in crate::checks::core_io::tests) fn rustflags_in_an_inline_table() -> Repo {
    good_crate_and(
        "rustflags-inline",
        &[(
            ".cargo/config.toml",
            "build = { jobs = 4, rustflags = [\"--cap-lints\", \"allow\"] }\n",
        )],
    )
}

/// A compiler key as a dotted key at the top of the file.
pub(in crate::checks::core_io::tests) fn compiler_key_as_a_dotted_key() -> Repo {
    good_crate_and(
        "rustc-wrapper-dotted",
        &[(".cargo/config.toml", "build.rustc-wrapper = \"./w.sh\"\n")],
    )
}

/// An alias as a dotted key at the top of the file.
pub(in crate::checks::core_io::tests) fn alias_as_a_dotted_key() -> Repo {
    good_crate_and(
        "alias-dotted",
        &[(
            ".cargo/config.toml",
            "alias.x = \"run\"\nalias.clippy = \"check\"\n",
        )],
    )
}

/// An `[env]` entry in an inline table.
pub(in crate::checks::core_io::tests) fn clippy_env_in_an_inline_table() -> Repo {
    good_crate_and(
        "clippy-env-inline",
        &[(".cargo/config.toml", "env = { CLIPPY_CONF_DIR = \"/\" }\n")],
    )
}

/// `workspace = false` is a `[lints]` table that does not inherit.
pub(in crate::checks::core_io::tests) fn crate_lints_workspace_false() -> Repo {
    good_crate_and(
        "crate-lints-false",
        &[(
            "crates/protocol/Cargo.toml",
            "[package]\nname = \"clauseo-protocol\"\n\n[lints]\nworkspace = false\n",
        )],
    )
}

/// A clippy config in another case: on a filesystem that ignores case, as macOS and Windows
/// have by default, clippy opens it as `clippy.toml`.
pub(in crate::checks::core_io::tests) fn clippy_toml_in_another_case() -> Repo {
    good_crate_and("clippy-toml-case", &[("crates/Clippy.toml", "")])
}

pub(in crate::checks::core_io::tests) fn dot_clippy_toml_in_another_case() -> Repo {
    good_crate_and(
        "dot-clippy-toml-case",
        &[(".CLIPPY.toml", "msrv = \"1.99\"\n")],
    )
}

/// The review's case: `include` loads a file core-io does not read.
pub(in crate::checks::core_io::tests) fn include_in_cargo_config() -> Repo {
    good_crate_and(
        "include",
        &[
            (
                ".cargo/config.toml",
                "include = [\"other.toml\"]\n\n[alias]\nx = \"run\"\n",
            ),
            (
                ".cargo/other.toml",
                "[build]\nrustflags = [\"--cap-lints\", \"allow\"]\n",
            ),
        ],
    )
}

/// The review's case: a wrapper that could add `--cap-lints allow`, and the other compiler keys.
pub(in crate::checks::core_io::tests) fn compiler_keys_in_cargo_config() -> Repo {
    good_crate_and(
        "rustc-wrapper",
        &[(
            ".cargo/config.toml",
            "[build]\njobs = 4\nrustc-wrapper = \"./w.sh\"\nrustc = \"rustc\"\n\
             rustc-workspace-wrapper = \"./w.sh\"\n",
        )],
    )
}

/// The repo's own `xtask` alias, the line as it is in `.cargo/config.toml`.
/// `the_xtask_alias_matches_the_cargo_config` fails when the two differ.
const XTASK_ALIAS: &str =
    "xtask = \"run --quiet --locked --package xtask --target-dir target/xtask-run --\"";

/// The review's case: an alias that turns clippy into `cargo check`, beside the repo's own
/// `xtask` alias.
pub(in crate::checks::core_io::tests) fn alias_of_a_command_xtask_runs() -> Repo {
    good_crate_and(
        "alias-clippy",
        &[(
            ".cargo/config.toml",
            &format!("[alias]\n{XTASK_ALIAS}\nclippy = \"check\"\n"),
        )],
    )
}

/// `alias_of_a_command_xtask_runs` puts the repo's own `xtask` alias beside the bad one, so its
/// copy has to stay the line in `.cargo/config.toml`.
#[test]
fn the_xtask_alias_matches_the_cargo_config() {
    let config = include_str!("../../../../../../.cargo/config.toml");
    assert!(
        config.lines().any(|line| line == XTASK_ALIAS),
        "XTASK_ALIAS is not a line of .cargo/config.toml: copy the `xtask` alias from there"
    );
}

/// A cargo config with aliases, an `[env]` entry and a build setting passes. cargo ignores an
/// alias named after a built-in command, such as `check`.
pub(in crate::checks::core_io::tests) fn cargo_config_without_lint_settings() -> Repo {
    good_crate_and(
        "cargo-config-clean",
        &[(
            ".cargo/config.toml",
            "[alias]\nx = \"run\"\ncheck = \"build\"\n\n[env]\nRUST_LOG = \"info\"\n\n[build]\njobs = 4\n",
        )],
    )
}
