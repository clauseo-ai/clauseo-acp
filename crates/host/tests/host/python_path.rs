//! The profile hook's session `PATH` folders (`ProfileHook::agent_path`, as desktop gives
//! Clauseo's Python runtime) on both agents' launches: after stetdoc's folder and before the
//! user's `PATH`, a relative one left off, and the inherited `PYTHONHOME` and `PYTHONPATH`
//! dropped only when folders go on. On macOS a Codex launch's `PATH` also has the folders
//! `path_helper` adds (`/etc/paths`, then `/etc/paths.d`) right after Clauseo's, each folder
//! once (`agent/shell_path.rs`); Claude's, Linux's and Windows's are Clauseo's, then the user's.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::Arc;

use clauseo_host::ProfileHook;
use clauseo_protocol::AgentId;
use serde_json::{Value, json};

use crate::profile_rig::{
    CLAUDE, CODEX, Wired, agent_lines, agents, codex_lines, settings, template, turn,
};
use crate::support::Dir;

/// A hook that puts `dirs` on every agent's session `PATH` (`ProfileHook::agent_path`, as
/// desktop does with Clauseo's Python runtime) and adds nothing else.
struct PathHook {
    dirs: Vec<PathBuf>,
}

impl ProfileHook for PathHook {
    fn session_meta(
        &self,
        _agent: &AgentId,
        _packs: Option<&clauseo_profile::PacksWritten>,
    ) -> Option<serde_json::Map<String, Value>> {
        None
    }

    fn launch_env(&self, _agent: &AgentId) -> BTreeMap<OsString, OsString> {
        BTreeMap::new()
    }

    fn agent_path(&self, _agent: &AgentId) -> Vec<PathBuf> {
        self.dirs.clone()
    }
}

/// One launch's `PATH` folders, `PYTHONHOME` and `PYTHONPATH`.
type HookLaunch = (Vec<PathBuf>, Option<OsString>, Option<OsString>);

/// Both agents' launches with stetdoc's folder and a [`PathHook`] over `dirs`, the inherited
/// environment's `PATH` being `/usr/bin` and its `PYTHONHOME` and `PYTHONPATH` set: each
/// launch's `PATH` folders, and its two Python variables.
fn launched_with_hook_path(label: &str, dirs: Vec<PathBuf>) -> (PathBuf, Vec<HookLaunch>) {
    let dir = Dir::new(label, &[]);
    let claude = template(
        &dir,
        "hook-path",
        &agent_lines("hook-path", "session/new", &json!({}), &[]),
    );
    let codex = template(
        &dir,
        "hook-path-codex",
        &codex_lines("hook-path-codex", &json!({})),
    );
    settings(
        &dir,
        agents(&[(CLAUDE, &claude, json!({})), (CODEX, &codex, json!({}))]),
        json!({}),
    );
    let ours = ["PATH", "PYTHONHOME", "PYTHONPATH"];
    let inherited: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(name, _)| !ours.iter().any(|own| name.eq_ignore_ascii_case(own)))
        .chain([
            ("PATH".into(), "/usr/bin".into()),
            ("PYTHONHOME".into(), "/elsewhere".into()),
            ("PYTHONPATH".into(), "/elsewhere/lib".into()),
        ])
        .collect();
    let folder = dir.data().join("stetdoc");
    let launcher = Wired::new(&dir);
    let host = dir.start_launching(0, Arc::<Wired>::clone(&launcher), |config| {
        config
            .with_agent_path_prepend(folder.clone())
            .with_inherited_env(inherited)
            .with_profile_hook(Box::new(PathHook { dirs }))
    });
    let matter = host.create_matter("m", &dir.folder("m", &[]));
    turn(&host, host.create_thread(matter, CLAUDE));
    turn(&host, host.create_thread(matter, CODEX));
    host.shutdown();
    let separator = if cfg!(windows) { ';' } else { ':' };
    let seen = launcher
        .seen()
        .into_iter()
        .map(|launch| {
            let (_, path) = launch
                .env
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
                .expect("a PATH");
            let parts = path
                .to_str()
                .unwrap()
                .split(separator)
                .map(PathBuf::from)
                .collect();
            let var = |name: &str| launch.env.get(&OsString::from(name)).cloned();
            (parts, var("PYTHONHOME"), var("PYTHONPATH"))
        })
        .collect();
    (folder, seen)
}

/// The `PATH` a launch gets with `ours` first and the user's `/usr/bin` after: on macOS a Codex
/// launch has the host's own `path_helper` folders between them, each folder kept where it
/// first comes, as `agent::shell_path::mac_shell_path` builds it.
fn expected_path(ours: &[PathBuf], codex: bool) -> Vec<PathBuf> {
    let helper = if codex && cfg!(target_os = "macos") {
        clauseo_host::path_helper_folders()
    } else {
        Vec::new()
    };
    let mut kept: Vec<PathBuf> = Vec::new();
    for part in ours
        .iter()
        .chain(&helper)
        .cloned()
        .chain([PathBuf::from("/usr/bin")])
    {
        if !part.as_os_str().is_empty() && !kept.contains(&part) {
            kept.push(part);
        }
    }
    kept
}

/// Whether launch `at` is Codex's: the rig runs Claude's turn, then Codex's.
fn is_codex(at: usize) -> bool {
    at == 1
}

/// The hook's folders go after stetdoc's and before the user's `PATH` on both agents, a
/// relative one left off, and the inherited `PYTHONHOME` and `PYTHONPATH` go.
#[test]
fn the_hook_path_follows_stetdoc_and_drops_the_python_variables() {
    let root = std::env::temp_dir().join("clauseo-python-rt");
    let bin = root.join("bin");
    let (folder, seen) = launched_with_hook_path(
        "tools_hook_path",
        vec![bin.clone(), PathBuf::from("relative/bin"), root.clone()],
    );
    assert_eq!(seen.len(), 2);
    for (at, (parts, home, path)) in seen.iter().enumerate() {
        let ours = [folder.clone(), bin.clone(), root.clone()];
        assert_eq!(parts, &expected_path(&ours, is_codex(at)), "launch {at}");
        assert_eq!(home, &None, "launch {at}");
        assert_eq!(path, &None, "launch {at}");
    }
}

/// A hook with no folders, or only relative ones, leaves `PATH` as before (stetdoc's, on macOS
/// Codex's `path_helper` folders, then the user's) and both Python variables inherited.
#[test]
fn an_empty_hook_path_keeps_the_path_and_the_python_variables() {
    for (label, dirs) in [
        ("tools_hook_path_empty", Vec::new()),
        (
            "tools_hook_path_relative",
            vec![PathBuf::from("relative/bin")],
        ),
    ] {
        let (folder, seen) = launched_with_hook_path(label, dirs);
        assert_eq!(seen.len(), 2, "{label}");
        for (at, (parts, home, path)) in seen.iter().enumerate() {
            let ours = [folder.clone()];
            assert_eq!(
                parts,
                &expected_path(&ours, is_codex(at)),
                "{label} launch {at}"
            );
            assert_eq!(
                home.as_deref(),
                Some(OsStr::new("/elsewhere")),
                "{label} {at}"
            );
            assert_eq!(
                path.as_deref(),
                Some(OsStr::new("/elsewhere/lib")),
                "{label} {at}"
            );
        }
    }
}
