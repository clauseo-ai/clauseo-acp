//! The scratch repos of the pins check's tests and planted cases. Every version here is made up:
//! the homes in each repo pin Rust 3.4.5, pnpm 7.8.9 and Node 6.7.0, and `TOOLS` and `BINARIES`
//! below pin cargo-fake 5.6.7 and lefthook 9.8.7, so no test repeats a real pin. The planted
//! harness runs the check with the real `TOOLS` and `BINARIES`, so a planted case never names
//! one of their versions; the cases that do are in `fake_pins.rs`.
//!
//! The builders of the workflow files, the lefthook comment and the forms that name a tool are
//! by topic in `repos/`, re-exported here so every case names `super::repos::<builder>`.

mod lefthook_comment;
mod tool_forms;
mod workflows;

pub(super) use lefthook_comment::*;
pub(super) use tool_forms::*;
pub(super) use workflows::*;

use super::super::inspect;
use crate::checks::Inspection;
use crate::checks::planted::{self, Repo};
use crate::scratch::Scratch;
use crate::setup::{Binary, Packing, Tool};
use crate::workspace::Context;
use std::path::Path;

/// The cargo tools the cases pin.
pub(super) const TOOLS: &[Tool] = &[Tool {
    name: "cargo-fake",
    version: "5.6.7",
}];

/// The binaries the cases pin.
pub(super) const BINARIES: &[Binary] = &[Binary {
    name: "lefthook",
    version: "9.8.7",
    sha256: &[],
    repo: "evilmartians/lefthook",
    winget: "evilmartians.lefthook",
    asset: |_, _, _| None,
    packing: Packing::Bare,
}];

/// The homes of a repo whose pins all read.
const TOOLCHAIN: &str = "[toolchain]\nchannel = \"3.4.5\"\n";
const PACKAGE_JSON: &str =
    "{ \"engines\": { \"node\": \">=6.7.0\" }, \"packageManager\": \"pnpm@7.8.9\" }\n";
/// A workspace manifest whose `rust-version` matches the toolchain.
const CARGO_TOML: &str = "[workspace.package]\nrust-version = \"3.4\"\n";
/// The three homes, by the path each is written to.
const HOMES: [(&str, &str); 3] = [
    ("rust-toolchain.toml", TOOLCHAIN),
    ("ui/package.json", PACKAGE_JSON),
    ("Cargo.toml", CARGO_TOML),
];

/// The homes, except the ones `changed` replaces, then `changed`.
fn homes<'a>(changed: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut files: Vec<(&'a str, &'a str)> = HOMES
        .into_iter()
        .filter(|(path, _)| !changed.iter().any(|(name, _)| name == path))
        .collect();
    files.extend_from_slice(changed);
    files
}

/// Writes `text` to `dir/rel`, making its folders.
pub(super) fn put(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        Scratch::create_dir_all(parent).expect("create the folders");
    }
    Scratch::write(path, text).expect("write the file");
}

/// The scratch repo of `case` with the homes, and `changed` in place of the files it names.
fn scratch_with_homes(case: &str, changed: &[(&str, &str)]) -> Scratch {
    planted::scratch(&format!("pins-{case}"), &homes(changed))
}

/// A scratch repo with the three homes and a matching `Cargo.toml`, for a unit test to write
/// into.
pub(super) fn repo(case: &str) -> Scratch {
    scratch_with_homes(case, &[])
}

/// Runs the check on `dir` with no workspace members.
pub(super) fn check(dir: &Path) -> Inspection {
    check_in(&Context {
        root: dir.to_path_buf(),
        members: Ok(Vec::new()),
        load_resolved_graph: false,
    })
}

/// Runs the check in `ctx` with the cases' `TOOLS` and `BINARIES`.
pub(super) fn check_in(ctx: &Context) -> Inspection {
    inspect(ctx, TOOLS, BINARIES)
}

/// A repo of the homes with `rel` written as `text`, and no workspace member.
fn with_file(case: &str, rel: &str, text: &str) -> Repo {
    Repo::files(&format!("pins-{case}"), &homes(&[(rel, text)]))
}

/// A repo whose `.github/workflows/ci.yml` is `text`.
fn workflow(case: &str, text: &str) -> Repo {
    with_file(case, ".github/workflows/ci.yml", text)
}

/// A repo whose `lefthook.yml` is `text`.
fn lefthook(case: &str, text: &str) -> Repo {
    with_file(case, "lefthook.yml", text)
}

/// A repo whose desktop crate's manifest is `text`, with the desktop crate as its member.
fn desktop_manifest(case: &str, text: &str) -> Repo {
    Repo::desktop(scratch_with_homes(
        case,
        &[("crates/desktop/Cargo.toml", text)],
    ))
}

// The Rust version.

pub(super) fn rust_version_matches() -> Repo {
    Repo::files("pins-rust-version-matches", &homes(&[]))
}

pub(super) fn rust_version_differs() -> Repo {
    with_file(
        "rust-version-differs",
        "Cargo.toml",
        "[workspace.package]\nedition = \"2024\"\nrust-version = \"3.3\"\n",
    )
}

pub(super) fn rust_version_in_a_dotted_key() -> Repo {
    with_file(
        "rust-version-dotted",
        "Cargo.toml",
        "[workspace]\nmembers = []\npackage.rust-version = \"3.5.0\"\n",
    )
}

pub(super) fn rust_version_in_a_member() -> Repo {
    desktop_manifest(
        "rust-version-member",
        "[package]\nname = \"clauseo-desktop\"\nrust-version = \"2.4\"\n",
    )
}

pub(super) fn rust_version_inherited_by_a_member() -> Repo {
    desktop_manifest(
        "rust-version-inherited",
        "[package]\nname = \"clauseo-desktop\"\nrust-version.workspace = true\n",
    )
}

pub(super) fn manifest_not_toml() -> Repo {
    with_file("manifest-not-toml", "Cargo.toml", "[workspace\n")
}

pub(super) fn toolchain_missing() -> Repo {
    Repo::files(
        "pins-toolchain-missing",
        &[
            ("ui/package.json", PACKAGE_JSON),
            ("Cargo.toml", CARGO_TOML),
        ],
    )
}

pub(super) fn channel_not_a_version() -> Repo {
    with_file(
        "channel-not-a-version",
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"stable\"\n",
    )
}

pub(super) fn package_manager_not_pnpm() -> Repo {
    with_file(
        "package-manager-not-pnpm",
        "ui/package.json",
        "{ \"engines\": { \"node\": \">=6.7.0\" }, \"packageManager\": \"yarn@1.2.3\" }",
    )
}

pub(super) fn engines_node_missing() -> Repo {
    with_file(
        "engines-node-missing",
        "ui/package.json",
        "{ \"packageManager\": \"pnpm@7.8.9+sha512.abc\" }",
    )
}

pub(super) fn rust_version_in_a_root_package() -> Repo {
    with_file(
        "rust-version-root-package",
        "Cargo.toml",
        "[package]\nname = \"root\"\nrust-version = \"3.3\"\n",
    )
}

pub(super) fn rust_version_in_a_quoted_member_key() -> Repo {
    desktop_manifest(
        "rust-version-quoted-member",
        "[\"package\"]\nname = \"clauseo-desktop\"\n\"rust-version\" = \"2.4\"\n",
    )
}

pub(super) fn rust_version_in_an_inline_member_table() -> Repo {
    desktop_manifest(
        "rust-version-inline-member",
        "package = { name = \"clauseo-desktop\", rust-version = \"2.4\" }\n",
    )
}

pub(super) fn engines_node_with_a_caret() -> Repo {
    with_file(
        "engines-node-caret",
        "ui/package.json",
        "{ \"engines\": { \"node\": \"^6.7.0\" }, \"packageManager\": \"pnpm@7.8.9\" }",
    )
}
