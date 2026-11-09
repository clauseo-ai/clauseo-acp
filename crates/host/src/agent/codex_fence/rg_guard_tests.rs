//! The ripgrep guard (`rg_guard.rs`, Linux): a missing `rg` and one in a folder the agent can
//! write refuse the Codex launch; the vendored `codex-path` copy comes first and is allowed.

use std::path::{Path, PathBuf};

use clauseo_protocol::ErrorCode;

use super::{Inputs, check, triple, vendor_folders};
use crate::pipeline::tests::support::TempDir;
use crate::write;

/// An executable (0755) `rg` in `folder`, made with its parents.
fn plant_rg(folder: &Path) -> PathBuf {
    write::create_dir_all(folder).unwrap();
    let rg = folder.join("rg");
    write::write_file(&rg, b"#!/bin/sh\n").unwrap();
    write::set_mode(&rg, 0o755).unwrap();
    rg
}

/// An install with the adapter's script and `@openai/codex`, and a matter, a run folder, notes
/// and a state folder.
struct World {
    dir: TempDir,
}

impl World {
    fn new(test: &str) -> World {
        let dir = TempDir::new(test);
        for folder in [
            "matter",
            "data/run/s1",
            "data/notes/m1",
            "data/agent-state/m1/codex-acp",
        ] {
            write::create_dir_all(&dir.path().join(folder)).unwrap();
        }
        let script = dir
            .path()
            .join("install/node_modules/@zed-industries/codex-acp/dist/index.js");
        write::create_dir_all(script.parent().unwrap()).unwrap();
        write::write_file(&script, b"").unwrap();
        World { dir }
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// The vendored `codex-path` of the install's `@openai/codex`.
    fn vendored(&self) -> PathBuf {
        self.at("install/node_modules/@openai/codex/vendor")
            .join(triple())
            .join("codex-path")
    }

    fn inputs(&self, path: &[PathBuf]) -> Inputs {
        Inputs {
            path: Some(std::env::join_paths(path).unwrap()),
            script: Some(self.at("install/node_modules/@zed-industries/codex-acp/dist/index.js")),
        }
    }

    fn check(&self, path: &[PathBuf]) -> Result<(), crate::error::Error> {
        let writable = [
            self.at("matter"),
            self.at("data/run/s1"),
            self.at("data/notes/m1"),
            self.at("data/agent-state/m1/codex-acp"),
        ];
        let writable: Vec<&Path> = writable.iter().map(PathBuf::as_path).collect();
        check(&self.inputs(path), &self.at("matter"), &writable)
    }
}

#[test]
fn a_missing_rg_refuses_the_launch() {
    let w = World::new("rg-guard-missing");
    let empty = w.at("bin-empty");
    write::create_dir_all(&empty).unwrap();
    let error = w.check(&[empty]).unwrap_err();
    assert_eq!(error.code(), ErrorCode::AgentLaunchFailed);
    assert!(error.to_string().contains("missing"), "{error}");
    assert!(!error.to_string().contains('/'), "no path: {error}");
}

/// An `rg` that is not executable is passed over, as a `PATH` lookup does.
#[test]
fn a_non_executable_rg_is_not_found() {
    let w = World::new("rg-guard-not-executable");
    let rg = plant_rg(&w.at("bin"));
    write::set_mode(&rg, 0o644).unwrap();
    assert!(w.check(&[w.at("bin")]).is_err());
}

/// The first `rg` on the path inside the matter, the run folder, the notes or the state folder
/// refuses, through a link too and from a relative `PATH` entry (resolved in the matter).
#[test]
fn an_rg_the_agent_can_write_refuses_the_launch() {
    let w = World::new("rg-guard-writable");
    for (folder, entry) in [
        ("matter/bin", w.at("matter/bin")),
        ("matter/rel", PathBuf::from("rel")),
        ("data/run/s1/tmp", w.at("data/run/s1/tmp")),
        ("data/notes/m1/tools", w.at("data/notes/m1/tools")),
        (
            "data/agent-state/m1/codex-acp/tmp",
            w.at("data/agent-state/m1/codex-acp/tmp"),
        ),
    ] {
        plant_rg(&w.at(folder));
        plant_rg(&w.at("usr/bin"));
        let error = w.check(&[entry, w.at("usr/bin")]).unwrap_err();
        assert!(
            error.to_string().contains("can change"),
            "{folder}: {error}"
        );
        write::remove_dir_all(&w.at(folder)).unwrap();
    }
    // A link elsewhere whose target is in the matter.
    plant_rg(&w.at("matter/hidden"));
    write::create_dir_all(&w.at("linked")).unwrap();
    write::symlink(&w.at("matter/hidden/rg"), &w.at("linked/rg")).unwrap();
    assert!(w.check(&[w.at("linked")]).is_err());
}

/// The vendored `codex-path` comes before the launch's `PATH`: its `rg` is allowed even with
/// one in the matter later on the path; a system `rg` alone is allowed too.
#[test]
fn the_vendored_rg_comes_first_and_is_allowed() {
    let w = World::new("rg-guard-vendored");
    plant_rg(&w.vendored());
    plant_rg(&w.at("matter/bin"));
    assert_eq!(
        vendor_folders(&w.at("install/node_modules/@zed-industries/codex-acp/dist/index.js")),
        [w.vendored()]
    );
    w.check(&[w.at("matter/bin")]).unwrap();

    let w = World::new("rg-guard-system");
    plant_rg(&w.at("usr/bin"));
    w.check(&[w.at("usr/bin")]).unwrap();
}

/// The adapter's own nested `node_modules` (`<adapter>/node_modules/@openai/codex-*`) is searched
/// before the install's, as Node finds the adapter's `@openai/codex`; its vendored `rg` is found
/// with no `rg` on the `PATH` and none in the install's `@openai/codex`.
#[test]
fn the_adapters_own_nested_codex_is_searched_first() {
    let w = World::new("rg-guard-nested");
    let nested = w
        .at("install/node_modules/@zed-industries/codex-acp/node_modules/@openai/codex-linux-x64")
        .join("vendor")
        .join(triple())
        .join("codex-path");
    plant_rg(&nested);
    let script = w.at("install/node_modules/@zed-industries/codex-acp/dist/index.js");
    assert_eq!(vendor_folders(&script), std::slice::from_ref(&nested));
    let empty = w.at("bin-empty");
    write::create_dir_all(&empty).unwrap();
    w.check(&[empty]).unwrap();
    // With the install's too, the nested one still comes first.
    plant_rg(&w.vendored());
    assert_eq!(vendor_folders(&script), [nested, w.vendored()]);
}
