//! The core-io planted cases about a held crate's target roots and build script: the forbid line
//! in each root, and no build script.

use super::{ARCHITECTURE, FORBIDDING_ROOT, LIB_ROOT, Repo, expected, run_repo};
use crate::checks::planted::Repo as ScratchRepo;

/// A target root without the forbid line.
const BARE_ROOT: &str = "//! A core crate.\npub fn f() {}\n";

/// The repo with the generated `crates/protocol/clippy.toml`, `files` and `targets`.
fn held(case: &str, files: &[(&str, &str)], targets: &[(&str, &str)]) -> ScratchRepo {
    let text = expected(case).text;
    run_repo(
        case,
        &Repo {
            crate_file: Some(&text),
            files,
            targets,
            ..Repo::default()
        },
    )
}

pub(in crate::checks::core_io) fn forbid_missing_in_the_lib() -> ScratchRepo {
    held("forbid-lib", &[(LIB_ROOT, BARE_ROOT)], &[])
}

/// The review's case: the line only under `cfg_attr`, which a cfg can switch off.
pub(in crate::checks::core_io) fn forbid_under_cfg_attr() -> ScratchRepo {
    held(
        "forbid-cfg-attr",
        &[(
            LIB_ROOT,
            "#![cfg_attr(all(), forbid(clippy::disallowed_methods, clippy::disallowed_types))]\n\
             pub fn f() {}\n",
        )],
        &[],
    )
}

/// The lints named in the other order are not the line the fix prints.
pub(in crate::checks::core_io) fn forbid_in_another_order() -> ScratchRepo {
    held(
        "forbid-order",
        &[(
            LIB_ROOT,
            "#![forbid(clippy::disallowed_types, clippy::disallowed_methods)]\npub fn f() {}\n",
        )],
        &[],
    )
}

pub(in crate::checks::core_io) fn forbid_missing_in_a_bin() -> ScratchRepo {
    let main = "crates/protocol/src/main.rs";
    held("forbid-bin", &[(main, BARE_ROOT)], &[("bin", main)])
}

/// The review's case: a test target, which clippy lints with `--all-targets`, turns a ban off at
/// its top.
pub(in crate::checks::core_io) fn forbid_missing_in_a_test() -> ScratchRepo {
    let test = "crates/protocol/tests/events.rs";
    held(
        "forbid-test",
        &[(
            test,
            "#![allow(clippy::disallowed_methods, reason = \"x\")]\n#[test]\nfn t() {}\n",
        )],
        &[("test", test)],
    )
}

pub(in crate::checks::core_io) fn root_not_rust() -> ScratchRepo {
    held(
        "root-not-rust",
        &[(
            LIB_ROOT,
            "#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]\n\npub fn 1() {}\n",
        )],
        &[],
    )
}

/// The review's case: a build script, whose `cargo::rustc-env` lines reach clippy.
pub(in crate::checks::core_io) fn build_rs_present() -> ScratchRepo {
    held(
        "build-rs",
        &[(
            "crates/protocol/build.rs",
            "fn main() {\n    println!(\"cargo::rustc-env=CLIPPY_CONF_DIR=/\");\n}\n",
        )],
        &[],
    )
}

pub(in crate::checks::core_io) fn package_build_set() -> ScratchRepo {
    held(
        "package-build",
        &[(
            "crates/protocol/Cargo.toml",
            "[package]\nname = \"clauseo-protocol\"\nbuild = \"gen.rs\"\n\n[lints]\nworkspace = true\n",
        )],
        &[],
    )
}

/// A row with `clock` alone needs no `clippy.toml`, but its crate still carries the line.
pub(in crate::checks::core_io) fn clock_only_crate_without_forbid() -> ScratchRepo {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "clock");
    run_repo(
        "clock-only-forbid",
        &Repo {
            architecture: Some(&architecture),
            files: &[(LIB_ROOT, BARE_ROOT)],
            ..Repo::default()
        },
    )
}

/// The review's case: `[[bin]] path` names a file outside the repo, which cargo builds.
pub(in crate::checks::core_io) fn root_outside_the_repo() -> ScratchRepo {
    held("root-outside", &[], &[("bin", "/elsewhere/outside.rs")])
}

/// The review's case: cargo metadata keeps the `..` of `[lib] path = "../../../x.rs"`.
pub(in crate::checks::core_io) fn root_outside_the_repo_through_dot_dot() -> ScratchRepo {
    held(
        "root-dot-dot",
        &[],
        &[("lib", "crates/protocol/../../../x.rs")],
    )
}

/// The review's case: a root in the repo but outside the crate's folder, whose files the cfg
/// rule does not read.
pub(in crate::checks::core_io) fn root_outside_the_crate() -> ScratchRepo {
    held(
        "root-outside-crate",
        &[("crates/shared/lib.rs", FORBIDDING_ROOT)],
        &[("lib", "crates/protocol/../shared/lib.rs")],
    )
}

/// The line in the root of every kind of target, and no build script, passes.
pub(in crate::checks::core_io) fn forbid_in_every_root() -> ScratchRepo {
    let roots = [
        ("bin", "crates/protocol/src/bin/tool.rs"),
        ("test", "crates/protocol/tests/events.rs"),
        ("example", "crates/protocol/examples/demo.rs"),
        ("bench", "crates/protocol/benches/fold.rs"),
    ];
    let files = roots.map(|(_, path)| (path, FORBIDDING_ROOT));
    held("forbid-every-root", &files, &roots)
}
