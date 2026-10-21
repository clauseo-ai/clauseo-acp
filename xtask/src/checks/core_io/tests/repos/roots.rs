//! The core-io planted cases about a held crate's target roots and build script: the forbid line
//! in each root, and no build script.

use super::{ARCHITECTURE, FORBIDDING_ROOT, LIB_ROOT, Layout, expected, run_repo};
use crate::checks::planted::Repo;

/// A target root without the forbid line.
const BARE_ROOT: &str = "//! A core crate.\npub fn f() {}\n";

/// The repo with the generated `crates/protocol/clippy.toml`, `files` and `targets`.
fn held(case: &str, files: &[(&str, &str)], targets: &[(&str, &str)]) -> Repo {
    let text = expected(case).text;
    run_repo(
        case,
        &Layout {
            crate_file: Some(&text),
            files,
            targets,
            ..Layout::default()
        },
    )
}

pub(in crate::checks::core_io::tests) fn forbid_missing_in_the_lib() -> Repo {
    held("forbid-lib", &[(LIB_ROOT, BARE_ROOT)], &[])
}

/// The review's case: the line only under `cfg_attr`, which a cfg can switch off.
pub(in crate::checks::core_io::tests) fn forbid_under_cfg_attr() -> Repo {
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
pub(in crate::checks::core_io::tests) fn forbid_in_another_order() -> Repo {
    held(
        "forbid-order",
        &[(
            LIB_ROOT,
            "#![forbid(clippy::disallowed_types, clippy::disallowed_methods)]\npub fn f() {}\n",
        )],
        &[],
    )
}

pub(in crate::checks::core_io::tests) fn forbid_missing_in_a_bin() -> Repo {
    let main = "crates/protocol/src/main.rs";
    held("forbid-bin", &[(main, BARE_ROOT)], &[("bin", main)])
}

/// The review's case: a test target, which clippy lints with `--all-targets`, turns a ban off at
/// its top.
pub(in crate::checks::core_io::tests) fn forbid_missing_in_a_test() -> Repo {
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

pub(in crate::checks::core_io::tests) fn forbid_missing_in_an_example() -> Repo {
    let example = "crates/protocol/examples/demo.rs";
    held(
        "forbid-example",
        &[(example, BARE_ROOT)],
        &[("example", example)],
    )
}

pub(in crate::checks::core_io::tests) fn forbid_missing_in_a_bench() -> Repo {
    let bench = "crates/protocol/benches/fold.rs";
    held("forbid-bench", &[(bench, BARE_ROOT)], &[("bench", bench)])
}

/// A forbid of one lint leaves the other free to be allowed.
pub(in crate::checks::core_io::tests) fn forbid_naming_one_lint() -> Repo {
    held(
        "forbid-one-lint",
        &[(
            LIB_ROOT,
            "#![forbid(clippy::disallowed_methods)]\npub fn f() {}\n",
        )],
        &[],
    )
}

/// `deny` in place of `forbid`: an `allow` in the target overrides it.
pub(in crate::checks::core_io::tests) fn deny_in_place_of_forbid() -> Repo {
    held(
        "deny-not-forbid",
        &[(
            LIB_ROOT,
            "#![deny(clippy::disallowed_methods, clippy::disallowed_types)]\npub fn f() {}\n",
        )],
        &[],
    )
}

/// The line at the top of an inline module covers that module, not the crate.
pub(in crate::checks::core_io::tests) fn forbid_inside_a_module() -> Repo {
    held(
        "forbid-in-module",
        &[(
            LIB_ROOT,
            "mod m {\n    #![forbid(clippy::disallowed_methods, clippy::disallowed_types)]\n}\n\
             pub fn f() {}\n",
        )],
        &[],
    )
}

/// The lints forbidden on one item, as an outer attribute, leave the rest of the crate free.
pub(in crate::checks::core_io::tests) fn forbid_as_an_outer_attribute() -> Repo {
    held(
        "forbid-outer",
        &[(
            LIB_ROOT,
            "#[forbid(clippy::disallowed_methods, clippy::disallowed_types)]\npub fn f() {}\n",
        )],
        &[],
    )
}

pub(in crate::checks::core_io::tests) fn root_not_rust() -> Repo {
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
pub(in crate::checks::core_io::tests) fn build_rs_present() -> Repo {
    held(
        "build-rs",
        &[(
            "crates/protocol/build.rs",
            "fn main() {\n    println!(\"cargo::rustc-env=CLIPPY_CONF_DIR=/\");\n}\n",
        )],
        &[],
    )
}

pub(in crate::checks::core_io::tests) fn package_build_set() -> Repo {
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
pub(in crate::checks::core_io::tests) fn clock_only_crate_without_forbid() -> Repo {
    let architecture = ARCHITECTURE.replace("tauri, fs, process", "clock");
    run_repo(
        "clock-only-forbid",
        &Layout {
            architecture: Some(&architecture),
            files: &[(LIB_ROOT, BARE_ROOT)],
            ..Layout::default()
        },
    )
}

/// The review's case: `[[bin]] path` names a file outside the repo, which cargo builds.
pub(in crate::checks::core_io::tests) fn root_outside_the_repo() -> Repo {
    held("root-outside", &[], &[("bin", "/elsewhere/outside.rs")])
}

/// The review's case: cargo metadata keeps the `..` of `[lib] path = "../../../x.rs"`.
pub(in crate::checks::core_io::tests) fn root_outside_the_repo_through_dot_dot() -> Repo {
    held(
        "root-dot-dot",
        &[],
        &[("lib", "crates/protocol/../../../x.rs")],
    )
}

/// The review's case: a root in the repo but outside the crate's folder, whose files the cfg
/// rule does not read.
pub(in crate::checks::core_io::tests) fn root_outside_the_crate() -> Repo {
    held(
        "root-outside-crate",
        &[("crates/shared/lib.rs", FORBIDDING_ROOT)],
        &[("lib", "crates/protocol/../shared/lib.rs")],
    )
}

/// The line in the root of every kind of target, and no build script, passes.
pub(in crate::checks::core_io::tests) fn forbid_in_every_root() -> Repo {
    let roots = [
        ("bin", "crates/protocol/src/bin/tool.rs"),
        ("test", "crates/protocol/tests/events.rs"),
        ("example", "crates/protocol/examples/demo.rs"),
        ("bench", "crates/protocol/benches/fold.rs"),
    ];
    let files = roots.map(|(_, path)| (path, FORBIDDING_ROOT));
    held("forbid-every-root", &files, &roots)
}
