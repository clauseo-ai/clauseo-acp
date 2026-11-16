//! Codex's data fence as the launch lists it (`Listed`): the own run folder, the shell
//! browser's folder and this matter's notes reachable to ordinary and escalated commands, other
//! sessions' and matters' denied, the tools folder and Clauseo's Python runtime readable, the
//! rest of `agents` denied, and the listing itself on disk.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::tests::{
    NODE, PYTHON, Turn, access, access_in, codex_home, data, fence, fence_with, listed, notes,
    readable, run_root, runtime,
};
use super::{Listed, PROFILE, list};

/// An approved escalated command's access to `path`.
fn escalated(keys: &Map<String, Value>, path: &Path) -> String {
    access_in(keys, Turn::Escalated, path)
}

/// The matter's brief in its notes.
fn brief(matter: &str) -> PathBuf {
    data()
        .join("agent-state")
        .join(matter)
        .join("notes")
        .join("matter-brief.md")
}

/// With the launch's list, the own run folder (the shell browser's `agent-browser` folder in it)
/// and this matter's notes (its brief) are writable to an ordinary command and to an escalated
/// one: no deny holds them. Another session's, another matter's, a set-aside run, the own
/// matter's scratch folder and `<data>/browser` stay denied to both.
#[test]
fn listed_own_folders_are_reachable_escalated_too_and_others_are_denied() {
    let keys = fence_with(&listed());
    let own = run_root().join("agent-browser");
    for path in [
        own.join("config.json"),
        own.join("profile").join("Default").join("Cookies"),
        own.join("downloads").join("judgment.pdf"),
        run_root().join("work").join("a.md"),
        brief("m1"),
    ] {
        assert_eq!(access(&keys, &path), "write", "{}", path.display());
        assert_eq!(escalated(&keys, &path), "write", "{}", path.display());
    }
    let data = data();
    for path in [
        data.join("run/s2/agent-browser/config.json"),
        data.join("run/s2/agent-browser/profile/Default/Cookies"),
        data.join("run/s3/agent-browser/downloads/judgment.pdf"),
        brief("m2"),
        data.join("agent-state/m1/scratch/x"),
        data.join("run.old-0/s9/work/a.txt"),
        data.join("browser"),
        data.join("browser/agent-browser.json"),
    ] {
        assert_eq!(access(&keys, &path), "deny", "{}", path.display());
        assert_eq!(escalated(&keys, &path), "deny", "{}", path.display());
    }
    let scoped = keys["permissions"][PROFILE]["filesystem"][data.to_str().unwrap()]
        .as_object()
        .unwrap();
    for whole in ["run", "agent-state", "agents/runtime", "agents"] {
        assert_eq!(scoped.get(whole), None, "{whole}");
    }
    assert_eq!(scoped.get("browser"), Some(&json!("deny")));
    for (subpath, mode) in scoped {
        if mode != "deny" {
            assert!(
                ["run/s1", "agent-state/m1/notes"].contains(&subpath.as_str()),
                "{subpath} is {mode}"
            );
        }
    }
}

/// The accepted residual (decision record 0030): a session or a matter made after the launch is
/// in no deny until the next launch; the same rules before any list deny them whole, and then an
/// escalated command reaches neither the own run folder nor the own notes.
#[test]
fn a_session_made_after_the_listed_launch_is_the_residual() {
    let later = data().join("run/s-later/work/a.txt");
    assert_eq!(access(&fence_with(&listed()), &later), "read");
    let unlisted = fence();
    assert_eq!(access(&unlisted, &later), "deny");
    assert_eq!(escalated(&unlisted, &run_root().join("work/a.md")), "deny");
    assert_eq!(escalated(&unlisted, &brief("m1")), "deny");
}

/// The commands the app copies for the agents (`<data>/tools`, agent-browser's copy) are
/// readable, so runnable, and not writable to an ordinary command.
#[test]
fn the_tools_folder_is_readable_and_not_writable() {
    for keys in [fence(), fence_with(&listed())] {
        let copy = data().join("tools").join("agent-browser");
        assert_eq!(access(&keys, &copy), "read");
        assert_eq!(access(&keys, &data().join("tools")), "read");
        assert_ne!(escalated(&keys, &copy), "deny");
    }
}

/// With a Python build listed, it is readable to an ordinary command (its `bin/python3`, `pip`
/// and `pandoc` run and read their library) and never writable; an escalated one may write
/// there (`:root` writable, the lawyer approved it). Node, a half-unpacked folder, a file beside
/// them, the sign-in shim, Claude's folder, another agent's and every named `agents` entry stay
/// denied to both.
#[test]
fn clauseo_python_is_readable_never_writable_and_its_siblings_stay_denied() {
    let keys = fence_with(&listed());
    let python = runtime().join(PYTHON);
    for path in [
        python.join("bin").join("python3"),
        python.join("bin").join("pip3"),
        python.join("bin").join("pandoc"),
        python.join("lib").join("python3.13").join("os.py"),
        python.clone(),
    ] {
        assert_eq!(access(&keys, &path), "read", "{}", path.display());
        assert_eq!(escalated(&keys, &path), "write", "{}", path.display());
    }
    let agents = data().join("agents");
    for path in [
        runtime().join(NODE).join("bin").join("node"),
        runtime()
            .join(format!(".tmp-{NODE}"))
            .join("bin")
            .join("node"),
        runtime().join("notes.txt"),
        agents.join("signin").join("shim"),
        agents.join("other-agent").join("install"),
        agents.join("downloads").join("x.tar.gz"),
        agents.join("npm-cache").join("x"),
        agents.join("npmrc"),
        agents.join("claude-acp").join("install").join("cli.js"),
        agents
            .join("claude-acp")
            .join("home")
            .join(".credentials.json"),
        codex_home().join("auth.json"),
    ] {
        assert_eq!(access(&keys, &path), "deny", "{}", path.display());
        assert_eq!(escalated(&keys, &path), "deny", "{}", path.display());
    }
    // Codex's own install and home stay readable.
    assert_eq!(
        access(&keys, &agents.join("codex-acp/install/codex")),
        "read"
    );
    let scoped = keys["permissions"][PROFILE]["filesystem"][data().to_str().unwrap()]
        .as_object()
        .unwrap();
    assert!(
        scoped
            .keys()
            .all(|subpath| !subpath.contains("clauseo-python"))
    );
}

/// Credentials are named whether or not the launch saw them: a sign-in shim, Claude's folder,
/// Codex's login made after a listed launch are denied, escalated too.
#[test]
fn credentials_made_after_the_launch_are_denied_by_name() {
    let empty = Listed {
        agents: Some(Vec::new()),
        runtime: Some(Vec::new()),
        ..listed()
    };
    let keys = fence_with(&empty);
    let agents = data().join("agents");
    for path in [
        agents.join("signin").join("shim"),
        agents
            .join("claude-acp")
            .join("home")
            .join(".credentials.json"),
        agents.join("npm-cache").join("x"),
        agents.join("downloads").join("x"),
        codex_home().join("auth.json"),
        codex_home().join("sessions").join("t.jsonl"),
    ] {
        assert_eq!(access(&keys, &path), "deny", "{}", path.display());
        assert_eq!(escalated(&keys, &path), "deny", "{}", path.display());
    }
}

/// With no Python build listed (none there, a list that failed, or a launch that did not
/// list), the runtime folder is denied whole, Python's build with it. A listed path outside the
/// folder it was listed from is ignored, never denied or opened, and so is one that holds an
/// own folder.
#[test]
fn unlisted_folders_are_denied_whole_and_stray_entries_ignored() {
    let keys = fence();
    let scoped = &keys["permissions"][PROFILE]["filesystem"][data().to_str().unwrap()];
    for whole in ["agents/runtime", "run", "agent-state"] {
        assert_eq!(scoped[whole], json!("deny"), "{whole}");
    }
    for path in [
        runtime().join(PYTHON).join("bin").join("python3"),
        runtime().join(NODE).join("bin").join("node"),
    ] {
        assert_eq!(access(&keys, &path), "deny", "{}", path.display());
    }
    let stray = Listed {
        run: Some(vec![data().join("profile"), run_root(), data().join("run")]),
        agent_state: Some(vec![notes(), data().join("agent-state").join("m1")]),
        runtime: Some(vec![data().join("profile").join("x")]),
        ..Listed::default()
    };
    let keys = fence_with(&stray);
    assert!(readable(&keys, &data().join("profile").join("x")));
    assert_eq!(escalated(&keys, &run_root().join("a")), "write");
    assert_eq!(escalated(&keys, &brief("m1")), "write");
}

/// The launch's list, on disk: the siblings of the own run folder and of the notes on the way
/// down, `run.old-*`, `agents` but Codex's folder and the runtime folder, and the runtime
/// folder but real Python build folders (a link with that name, a file and Node are listed).
/// No Python build, or a folder on the way that is a link, leaves that part unlisted.
#[test]
fn the_launch_list_names_siblings_only() {
    use crate::pipeline::tests::support::TempDir;
    let dir = TempDir::new("codex-fence-list");
    let data = dir.path().join("data");
    let (run_root, notes) = (data.join("run/s1"), data.join("agent-state/m1/notes"));
    let runtime = data.join("agents/runtime");
    for folder in [
        run_root.join("agent-browser"),
        data.join("run/s2"),
        notes.clone(),
        data.join("agent-state/m1/scratch"),
        data.join("agent-state/m2/notes"),
        data.join("run.old-7/s9"),
        data.join("agents/codex-acp/home"),
        data.join("agents/claude-acp/home"),
        runtime.join(NODE),
    ] {
        crate::write::create_dir_all(&folder).unwrap();
    }
    crate::write::write_file(&runtime.join("clauseo-python-file"), b"x").unwrap();
    let got = list(&data, &run_root, &notes);
    assert_eq!(got.run, Some(vec![data.join("run/s2")]));
    assert_eq!(
        got.agent_state,
        Some(vec![
            data.join("agent-state/m2"),
            data.join("agent-state/m1/scratch"),
        ])
    );
    assert_eq!(got.set_aside, vec![data.join("run.old-7")]);
    assert_eq!(got.agents, Some(vec![data.join("agents/claude-acp")]));
    assert_eq!(got.runtime, None);
    crate::write::create_dir_all(&runtime.join(PYTHON).join("bin")).unwrap();
    crate::write::create_dir_all(&runtime.join("clauseo-python-")).unwrap();
    let mut want = vec![
        runtime.join(NODE),
        runtime.join("clauseo-python-"),
        runtime.join("clauseo-python-file"),
    ];
    #[cfg(unix)]
    {
        let elsewhere = dir.path().join("elsewhere");
        crate::write::create_dir_all(&elsewhere).unwrap();
        crate::write::symlink(&elsewhere, &runtime.join("clauseo-python-link")).unwrap();
        want.push(runtime.join("clauseo-python-link"));
        crate::write::symlink(&elsewhere, &data.join("run/s1/../s-link")).unwrap();
    }
    want.sort();
    let got = list(&data, &run_root, &notes);
    assert_eq!(got.runtime, Some(want));
    #[cfg(unix)]
    {
        assert_eq!(
            got.run,
            Some(vec![data.join("run/s-link"), data.join("run/s2")])
        );
        let linked = dir.path().join("linked");
        crate::write::create_dir_all(&linked.join("agents")).unwrap();
        crate::write::create_dir_all(&linked.join("agent-state")).unwrap();
        crate::write::symlink(&runtime, &linked.join("agents/runtime")).unwrap();
        crate::write::symlink(&data.join("run"), &linked.join("run")).unwrap();
        let got = list(
            &linked,
            &linked.join("run/s1"),
            &linked.join("agent-state/m1/notes"),
        );
        assert_eq!((got.run, got.agent_state, got.runtime), (None, None, None));
    }
}
