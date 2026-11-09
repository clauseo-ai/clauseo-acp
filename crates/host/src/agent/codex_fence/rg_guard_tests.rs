//! The ripgrep guard (`rg_guard.rs`, Linux) on fake installs: the bundled `rg` of a pinned
//! install, found in Node's order (nested `node_modules` first), passes; a missing package, a
//! version not pinned, a missing, non-executable, linked or changed `rg`, and one whose real path
//! is inside a folder the agent can write refuse, each with its words and no path.

use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;
use sha2::Digest as _;

use super::{ADAPTER, Inputs, LAYOUT_FILE, Pins, check, node_lookup, platform, shipped_layout};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// The fake `rg`'s bytes.
pub(super) const RG: &[u8] = b"#!/bin/sh\necho ripgrep\n";

/// The fake install's pins: its versions, and [`RG`]'s hash.
pub(super) fn pins() -> Pins {
    Pins {
        codex_acp: "9.0.0-test",
        codex: "1.2.3-test",
        rg_sha256: sha2::Sha256::digest(RG).into(),
    }
}

/// This machine's npm platform name (`linux-x64`, `linux-arm64`).
fn npm() -> &'static str {
    platform().expect("a processor Codex ships for").npm
}

/// Writes `folder/package.json` naming `name` at `version`, making the folder.
fn package(folder: &Path, name: &str, version: &str) {
    write::create_dir_all(folder).unwrap();
    let text = serde_json::json!({ "name": name, "version": version }).to_string();
    write::write_file(&folder.join("package.json"), text.as_bytes()).unwrap();
}

/// An install under `<root>/<install>` with the adapter's package and script, and a matter, a
/// run folder, notes and a state folder beside it.
pub(super) struct World {
    dir: TempDir,
    install: String,
}

impl World {
    /// An install at `install` holding only the adapter at the pinned version.
    pub(super) fn bare(test: &str, install: &str) -> World {
        let dir = TempDir::new(test);
        for folder in [
            "matter",
            "data/run/s1",
            "data/notes/m1",
            "data/agent-state/m1/codex-acp",
        ] {
            write::create_dir_all(&dir.path().join(folder)).unwrap();
        }
        let world = World {
            dir,
            install: install.to_owned(),
        };
        package(&world.adapter(), ADAPTER, pins().codex_acp);
        let script = world.script();
        write::create_dir_all(script.parent().unwrap()).unwrap();
        write::write_file(&script, b"").unwrap();
        world
    }

    /// A pinned install at `install`: the adapter, `@openai/codex` and the platform package
    /// with its `rg` in the install's `node_modules`.
    pub(super) fn pinned(test: &str, install: &str) -> World {
        let world = World::bare(test, install);
        World::codex(&world.at(install), pins().codex);
        World::build(&world.at(install), &format!("{}-{}", pins().codex, npm()));
        world
    }

    pub(super) fn at(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// The adapter's package folder.
    fn adapter(&self) -> PathBuf {
        self.at(&self.install)
            .join("node_modules/@agentclientprotocol/codex-acp")
    }

    /// The adapter's script, `<adapter>/dist/index.js`.
    pub(super) fn script(&self) -> PathBuf {
        self.adapter().join("dist/index.js")
    }

    /// `@openai/codex` at `version` in `base`'s `node_modules`; its folder.
    fn codex(base: &Path, version: &str) -> PathBuf {
        let folder = base.join("node_modules/@openai/codex");
        package(&folder, "@openai/codex", version);
        folder
    }

    /// The platform package at `version` in `base`'s `node_modules` (named `@openai/codex`, as
    /// npm's alias install names it), with an executable [`RG`] and the layout file Codex ships
    /// at the pinned version ([`write_layout`]); the `rg`'s path.
    fn build(base: &Path, version: &str) -> PathBuf {
        let folder = base.join(format!("node_modules/@openai/codex-{}", npm()));
        package(&folder, "@openai/codex", version);
        let vendored = folder
            .join("vendor")
            .join(platform().unwrap().triple)
            .join("codex-path");
        write::create_dir_all(&vendored).unwrap();
        let rg = vendored.join("rg");
        write::write_file(&rg, RG).unwrap();
        write::set_mode(&rg, 0o755).unwrap();
        write_layout(vendored.parent().unwrap(), pins().codex);
        rg
    }

    /// The guard on `inputs`, the agent running in the matter, with the matter, the run folder,
    /// the notes and the state folder writable.
    pub(super) fn check_inputs(&self, inputs: &Inputs) -> Result<(), crate::error::Error> {
        let writable = [
            self.at("matter"),
            self.at("data/run/s1"),
            self.at("data/notes/m1"),
            self.at("data/agent-state/m1/codex-acp"),
        ];
        let writable: Vec<&Path> = writable.iter().map(PathBuf::as_path).collect();
        check(inputs, &self.at("matter"), &writable)
    }

    /// The guard on this install's script with [`pins`].
    pub(super) fn check(&self) -> Result<(), crate::error::Error> {
        self.check_inputs(&Inputs {
            script: Some(self.script()),
            pins: Some(pins()),
        })
    }
}

/// Writes `folder/codex-package.json` as Codex `version` ships it for this machine.
fn write_layout(folder: &Path, version: &str) {
    let text = shipped_layout(version, platform().unwrap().triple).to_string();
    write::write_file(&folder.join(LAYOUT_FILE), text.as_bytes()).unwrap();
}

/// Asserts `result` is an `agent.launch_failed` refusal holding `words` and no path.
#[track_caller]
pub(super) fn refused(result: Result<(), crate::error::Error>, words: &str) {
    let error = result.unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    let text = error.to_string();
    assert!(text.contains(words), "{text}");
    assert!(!text.contains('/'), "no path: {text}");
}

/// The words of each refusal.
pub(super) const MISSING: &str = "(ripgrep) is missing. Reinstall Codex";
const VERSION: &str = "version isn't the one this Clauseo was checked with. Reinstall Codex";
const NOT_SHIPPED: &str = "(ripgrep) isn't the one Codex ships. Reinstall Codex";
const WRITABLE: &str = "a folder the agent can change";
const LAYOUT: &str = "layout file (codex-package.json) is missing or isn't the one Codex ships";

#[test]
fn the_pinned_install_passes() {
    let w = World::pinned("rg-guard-pinned", "install");
    w.check().unwrap();
    // A relative script resolves in the folder the agent runs in.
    let w = World::pinned("rg-guard-relative", "matter-sibling");
    w.check_inputs(&Inputs {
        script: Some(PathBuf::from(
            "../matter-sibling/node_modules/@agentclientprotocol/codex-acp/dist/index.js",
        )),
        pins: Some(pins()),
    })
    .unwrap();
}

/// The adapter's own nested `@openai/codex` is the one Node loads, before the install's; and
/// that package's own nested platform package before the one further up.
#[test]
fn nested_packages_come_first_in_nodes_order() {
    let w = World::pinned("rg-guard-nested", "install");
    let nested = World::codex(&w.adapter(), "0.0.1");
    assert_eq!(
        node_lookup(&w.adapter(), "codex"),
        Some(dunce::canonicalize(&nested).unwrap())
    );
    refused(w.check(), VERSION);
    // Pinned, it finds the install's platform package above it.
    World::codex(&w.adapter(), pins().codex);
    w.check().unwrap();
    // Its own nested platform package comes first: a changed `rg` there refuses.
    let rg = World::build(&nested, &format!("{}-{}", pins().codex, npm()));
    write::write_file(&rg, b"#!/bin/sh\necho changed\n").unwrap();
    refused(w.check(), NOT_SHIPPED);
    write::write_file(&rg, RG).unwrap();
    w.check().unwrap();
}

#[test]
fn a_missing_package_refuses() {
    // No adapter package above the script, or no script at all.
    let w = World::pinned("rg-guard-no-adapter", "install");
    package(&w.adapter(), "@zed-industries/codex-acp", pins().codex_acp);
    refused(w.check(), MISSING);
    let none = Inputs {
        script: None,
        pins: Some(pins()),
    };
    refused(w.check_inputs(&none), MISSING);
    let gone = Inputs {
        script: Some(w.at("nowhere/index.js")),
        pins: Some(pins()),
    };
    refused(w.check_inputs(&gone), MISSING);
    // No `@openai/codex`.
    let w = World::bare("rg-guard-no-codex", "install");
    refused(w.check(), MISSING);
    // No platform package.
    World::codex(&w.at("install"), pins().codex);
    refused(w.check(), MISSING);
}

#[test]
fn a_version_not_pinned_refuses() {
    let w = World::pinned("rg-guard-version", "install");
    package(&w.adapter(), ADAPTER, "2.2.1");
    refused(w.check(), VERSION);
    package(&w.adapter(), ADAPTER, pins().codex_acp);
    World::codex(&w.at("install"), "0.160.1");
    refused(w.check(), VERSION);
    World::codex(&w.at("install"), pins().codex);
    // The other processor's build, or the bare version.
    for version in [
        format!("{}-linux-riscv", pins().codex),
        pins().codex.to_owned(),
    ] {
        package(
            &w.at(&format!("install/node_modules/@openai/codex-{}", npm())),
            "@openai/codex",
            &version,
        );
        refused(w.check(), VERSION);
    }
}

#[test]
fn a_missing_or_changed_rg_refuses() {
    let w = World::pinned("rg-guard-rg", "install");
    let rg = w
        .at(&format!("install/node_modules/@openai/codex-{}", npm()))
        .join("vendor")
        .join(platform().unwrap().triple)
        .join("codex-path/rg");
    // Changed bytes.
    write::write_file(&rg, b"#!/bin/sh\necho other\n").unwrap();
    refused(w.check(), NOT_SHIPPED);
    write::write_file(&rg, RG).unwrap();
    // Not executable.
    write::set_mode(&rg, 0o644).unwrap();
    refused(w.check(), NOT_SHIPPED);
    // A link to a file with the pinned bytes.
    write::remove_file(&rg).unwrap();
    let elsewhere = w.at("elsewhere/rg");
    write::create_dir_all(elsewhere.parent().unwrap()).unwrap();
    write::write_file(&elsewhere, RG).unwrap();
    write::set_mode(&elsewhere, 0o755).unwrap();
    write::symlink(&elsewhere, &rg).unwrap();
    refused(w.check(), NOT_SHIPPED);
    // A folder.
    write::remove_file(&rg).unwrap();
    write::create_dir_all(&rg).unwrap();
    refused(w.check(), NOT_SHIPPED);
    // None.
    write::remove_dir_all(&rg).unwrap();
    refused(w.check(), MISSING);
}

/// A pinned install inside a folder the agent can write refuses, as does one whose platform
/// package is a link into the matter (the canonical path is compared).
#[test]
fn an_install_the_agent_can_write_refuses() {
    for install in [
        "matter/tools",
        "data/run/s1/tmp",
        "data/agent-state/m1/codex-acp/tmp",
    ] {
        let w = World::pinned("rg-guard-writable", install);
        refused(w.check(), WRITABLE);
    }
    let w = World::pinned("rg-guard-linked", "install");
    let real = w.at("matter/hidden");
    let build = w.at(&format!("install/node_modules/@openai/codex-{}", npm()));
    write::create_dir_all(real.parent().unwrap()).unwrap();
    write::rename(&build, &real).unwrap();
    write::symlink(&real, &build).unwrap();
    refused(w.check(), WRITABLE);
}

/// Round 3b: Codex finds `codex-path` only through `codex-package.json` beside `bin/`, so
/// the guard checks it: the shipped file passes; one missing, a link (even to the shipped
/// bytes), a folder, at another version or target, with a field more, or not JSON refuses with
/// its own words and no path; and one whose real path is in the matter refuses as writable.
#[test]
fn the_layout_file_must_be_the_one_codex_ships() {
    let w = World::pinned("rg-guard-layout", "install");
    let folder = w
        .at(&format!("install/node_modules/@openai/codex-{}", npm()))
        .join("vendor")
        .join(platform().unwrap().triple);
    let file = folder.join(LAYOUT_FILE);
    w.check().unwrap();
    // Missing.
    write::remove_file(&file).unwrap();
    refused(w.check(), LAYOUT);
    // A link to a file with the shipped bytes, outside any writable folder.
    let elsewhere = w.at("elsewhere");
    write::create_dir_all(&elsewhere).unwrap();
    write_layout(&elsewhere, pins().codex);
    write::symlink(&elsewhere.join(LAYOUT_FILE), &file).unwrap();
    refused(w.check(), LAYOUT);
    write::remove_file(&file).unwrap();
    // A folder.
    write::create_dir_all(&file).unwrap();
    refused(w.check(), LAYOUT);
    write::remove_dir_all(&file).unwrap();
    // Another version or target, a field more, not JSON.
    let triple = platform().unwrap().triple;
    let mut extra = shipped_layout(pins().codex, triple);
    extra["codexHome"] = serde_json::json!("/tmp");
    for wrong in [
        shipped_layout("0.160.1", triple).to_string(),
        shipped_layout(pins().codex, "riscv64-unknown-linux-musl").to_string(),
        extra.to_string(),
        "not json".to_owned(),
    ] {
        write::write_file(&file, wrong.as_bytes()).unwrap();
        refused(w.check(), LAYOUT);
    }
    // The shipped file again passes.
    write_layout(&folder, pins().codex);
    w.check().unwrap();
}

#[path = "rg_guard_pins_tests.rs"]
mod pins_tests;
