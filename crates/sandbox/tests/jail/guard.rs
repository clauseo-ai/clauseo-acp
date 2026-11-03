//! The guard sidecar in an app bundle, run the way Claude Code runs a hook: `/bin/sh -c
//! '<path>'` with one JSON object on stdin. The stand-in is a copy of `fake_agent` in its
//! `--scenario hook` mode, at `Clauseo.app/Contents/MacOS/clauseo-guard`, with a second copy
//! beside it as `Clauseo`, the desktop app. The base profile lets any program run but reads
//! files only in system locations, so the guard runs with no grant, and a grant on the guard
//! file opens nothing else in its folder.

use std::path::{Path, PathBuf};

use crate::support::{World, launcher, sh};

/// The hook input Claude Code sends for a Write into the matter, as one line.
fn hook_input(matter: &Path) -> String {
    format!(
        r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}/a.docx"}}}}"#,
        matter.display()
    )
}

/// The script that runs `guard` as a hook with `input` on stdin. The stand-in's hook mode is
/// `--scenario hook`, and `sh -c` carries that argument; the real guard takes none.
fn hook_script(guard: &Path, input: &str) -> String {
    format!(
        "printf '%s' '{input}' | '{}' --scenario hook",
        guard.display()
    )
}

/// Lays out `Clauseo.app/Contents/MacOS/` with `fake_agent` copied in as the guard and as
/// `Clauseo`. Returns the guard's path.
fn bundle(w: &World, bin: &Path) -> PathBuf {
    let guard = w.copy_in(bin, "Clauseo.app/Contents/MacOS/clauseo-guard");
    w.copy_in(bin, "Clauseo.app/Contents/MacOS/Clauseo");
    guard
}

#[test]
fn guard_sidecar_runs_from_an_app_bundle_under_a_per_file_runtime_grant() {
    let w = World::new();
    let bin = clauseo_fake_agent::bin_path()
        .expect("fake_agent binary: run `cargo build -p clauseo-fake_agent` first");
    let guard = bundle(&w, &bin);
    let policy = w.builder().runtime(&guard).build().unwrap();
    let (launcher, _) = launcher(policy);
    let matter = w.at("m1");
    let input = hook_input(&matter);
    let out = sh(&launcher, &matter, &hook_script(&guard, &input), &[]);
    out.assert_ok();
    assert_eq!(out.stdout, format!("hook ok {}\n", input.len()));
    // The grant is the guard file alone: its sibling and its folder stay refused.
    let sibling = guard.with_file_name("Clauseo");
    let folder = guard.parent().unwrap();
    let cat = format!("cat '{}'", sibling.display());
    sh(&launcher, &matter, &cat, &[]).assert_refused();
    let ls = format!("ls '{}'", folder.display());
    sh(&launcher, &matter, &ls, &[]).assert_refused();
}

// Tripwire: base.sb lets any program run (`(allow process-exec)`) while refusing to read it.
// If base.sb ever narrows `process-exec`, this test fails, and the host's policy must name the
// guard with `.runtime(path)`, which the test above covers.
#[test]
fn guard_sidecar_runs_without_a_grant_through_base_process_exec() {
    let w = World::new();
    let bin = clauseo_fake_agent::bin_path()
        .expect("fake_agent binary: run `cargo build -p clauseo-fake_agent` first");
    let guard = bundle(&w, &bin);
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let matter = w.at("m1");
    let input = hook_input(&matter);
    let out = sh(&launcher, &matter, &hook_script(&guard, &input), &[]);
    out.assert_ok();
    assert_eq!(out.stdout, format!("hook ok {}\n", input.len()));
    sh(
        &launcher,
        &matter,
        &format!("cat '{}'", guard.display()),
        &[],
    )
    .assert_refused();
}
