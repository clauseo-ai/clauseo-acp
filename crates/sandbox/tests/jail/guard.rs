//! The pre-write guard hook in an app bundle, run the way Claude Code runs a hook: `/bin/sh -c
//! '<path>'` with one JSON object on stdin. The real hook is the desktop exe itself, run as
//! `<desktop exe> --pre-write-guard`; the `clauseo-guard` sidecar is parked, so nothing here is
//! named that. The stand-in is a copy of `fake_agent` in its `--scenario hook` mode, at
//! `Clauseo.app/Contents/MacOS/Clauseo`, the desktop exe, with a second copy beside it as
//! `clauseo-mcp`, an executable the policy does not name. The base profile lets any program run
//! but reads files only in system locations, so the hook runs with no grant, and a grant on the
//! desktop exe file opens nothing else in its folder.

use std::path::{Path, PathBuf};

use crate::support::{World, launcher, sh};

/// The hook input Claude Code sends for a Write into the matter, as one line.
fn hook_input(matter: &Path) -> String {
    format!(
        r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}/a.docx"}}}}"#,
        matter.display()
    )
}

/// The script that runs `exe` as a hook with `input` on stdin. The stand-in's hook mode is
/// `--scenario hook`, and `sh -c` carries that argument; the real hook takes `--pre-write-guard`.
fn hook_script(exe: &Path, input: &str) -> String {
    format!(
        "printf '%s' '{input}' | '{}' --scenario hook",
        exe.display()
    )
}

/// Lays out `Clauseo.app/Contents/MacOS/` with `fake_agent` copied in as `Clauseo` and as the
/// sibling `clauseo-mcp`. Returns the desktop exe's path.
fn bundle(w: &World, bin: &Path) -> PathBuf {
    let exe = w.copy_in(bin, "Clauseo.app/Contents/MacOS/Clauseo");
    w.copy_in(bin, "Clauseo.app/Contents/MacOS/clauseo-mcp");
    exe
}

#[test]
fn desktop_exe_runs_as_the_hook_from_an_app_bundle_under_a_per_file_runtime_grant() {
    let w = World::new();
    let bin = clauseo_fake_agent::bin_path()
        .expect("fake_agent binary: run `cargo build -p clauseo-fake_agent` first");
    let exe = bundle(&w, &bin);
    let policy = w.builder().runtime(&exe).build().unwrap();
    let (launcher, _) = launcher(policy);
    let matter = w.at("m1");
    let input = hook_input(&matter);
    let out = sh(&launcher, &matter, &hook_script(&exe, &input), &[]);
    out.assert_ok();
    assert_eq!(out.stdout, format!("hook ok {}\n", input.len()));
    // The grant is the desktop exe file alone: its sibling and its folder stay refused.
    let sibling = exe.with_file_name("clauseo-mcp");
    let folder = exe.parent().unwrap();
    let cat = format!("cat '{}'", sibling.display());
    sh(&launcher, &matter, &cat, &[]).assert_refused();
    let ls = format!("ls '{}'", folder.display());
    sh(&launcher, &matter, &ls, &[]).assert_refused();
}

// Tripwire: base.sb lets any program run (`(allow process-exec)`) while refusing to read it.
// If base.sb ever narrows `process-exec`, this test fails, and the host's policy must name the
// desktop exe with `.runtime(path)`, which the test above covers.
#[test]
fn desktop_exe_runs_as_the_hook_without_a_grant_through_base_process_exec() {
    let w = World::new();
    let bin = clauseo_fake_agent::bin_path()
        .expect("fake_agent binary: run `cargo build -p clauseo-fake_agent` first");
    let exe = bundle(&w, &bin);
    let (launcher, _) = launcher(w.builder().build().unwrap());
    let matter = w.at("m1");
    let input = hook_input(&matter);
    let out = sh(&launcher, &matter, &hook_script(&exe, &input), &[]);
    out.assert_ok();
    assert_eq!(out.stdout, format!("hook ok {}\n", input.len()));
    sh(&launcher, &matter, &format!("cat '{}'", exe.display()), &[]).assert_refused();
}
