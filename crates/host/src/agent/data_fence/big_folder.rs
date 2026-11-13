//! For tests: a data folder the size of a busy lawyer's, as a real launch enumerates it
//! (`launch.rs`): 5 run folders (one, `…-1`, a text prefix of the session's own `…-10`), 60
//! matters with notes, Claude's home with a project for each matter's folder, the logs, the
//! stores, the settings file and the agents' folders.

use std::path::{Path, PathBuf};

use super::{LaunchDeny, claude_projects, launch_deny};
use crate::write;

/// How many matters the folder holds.
pub(crate) const MATTERS: usize = 60;

/// The session's own run folder's name: `<uuid>-<n>`, as `SessionKey::id` makes it.
pub(crate) const OWN_RUN: &str = "01a1d5be-5efa-73f6-a24b-dc90c01883d9-10";

/// Another run of the same thread, whose name is a text prefix of [`OWN_RUN`].
pub(crate) const PREFIX_RUN: &str = "01a1d5be-5efa-73f6-a24b-dc90c01883d9-1";

/// The `n`th matter's id (from 0); the session's matter is the 0th.
pub(crate) fn matter_id(n: usize) -> String {
    format!("01a1d5b4-a291-7649-9fa4-{n:012x}")
}

/// The `n`th matter's folder, as a lawyer's Windows machine names it (the project keys only).
pub(crate) fn matter_folder(n: usize) -> String {
    format!(r"C:\Users\clauseo-lawyer\CloudProxy\Matter {n:02}")
}

/// A launch's deny rules in the big folder, and the session's own folders.
pub(crate) struct Big {
    /// `<root>/data`.
    pub(crate) data: PathBuf,
    /// The launch's rules, `data` its one spelling.
    pub(crate) deny: LaunchDeny,
    /// `data/run/<OWN_RUN>`.
    pub(crate) run: PathBuf,
    /// `data/agent-state/<matter 0>/notes`.
    pub(crate) notes: PathBuf,
}

/// Makes the big folder under `root` and enumerates it for a launch of matter 0's session
/// [`OWN_RUN`].
pub(crate) fn big(root: &Path) -> Big {
    big_with(root, MATTERS, MATTERS)
}

/// [`big`] with `matters` matters' notes and `project_count` Claude projects (one per matter folder
/// `0..project_count`), at least one of each for the session's own.
pub(crate) fn big_with(root: &Path, matters: usize, project_count: usize) -> Big {
    let data = root.join("data");
    let mut files: Vec<String> = [
        "settings.json",
        "state.sqlite",
        "state.sqlite-wal",
        "search.sqlite",
        "update.json",
        "logs/host.log",
        "counts/2026-11.json",
        "tools/agent-browser/agent-browser",
        "profile/claude-plugin/skills/legal-drafting/SKILL.md",
        "agents/codex-acp/home/auth.json",
        "agents/claude-acp/install/package.json",
        "agents/claude-acp/login-source",
        "agents/claude-acp/home/.credentials.json",
        "agents/claude-acp/home/.claude.json",
        "agents/claude-acp/home/settings.json",
        "agents/claude-acp/home/history.jsonl",
        "agents/claude-acp/home/plans/plan.md",
        "agents/claude-acp/home/todos/t.json",
        "agents/claude-acp/home/statsig/s",
        "agents/runtime/node-v24.1.0/bin/node",
        "agents/runtime/clauseo-python-3.13.18-c2-windows-x64/python.exe",
        "agents/signin/shim.js",
        "launch-settings/other-run.json",
    ]
    .iter()
    .map(|path| (*path).to_owned())
    .collect();
    for n in [1, 2, 3, 4] {
        let name = if n == 1 {
            PREFIX_RUN.to_owned()
        } else {
            format!("01a1d5c4-90d5-7548-96d0-4a161074492{n}-{n}")
        };
        files.push(format!("run/{name}/work/draft.md"));
        files.push(format!("run/{name}/relay-token"));
    }
    for own in [
        "work/reply.md",
        "relay-token",
        "skill-packs/p/x",
        "clauseo-env.sh",
    ] {
        files.push(format!("run/{OWN_RUN}/{own}"));
    }
    let home = data.join("agents/claude-acp/home");
    let projects = |n: usize| {
        let key = super::claude_project_key(&matter_folder(n));
        format!("agents/claude-acp/home/projects/{key}")
    };
    for n in 0..matters.max(1) {
        files.push(format!("agent-state/{}/notes/notes.md", matter_id(n)));
    }
    for n in 0..project_count.max(1) {
        files.push(format!("{}/0193-session.jsonl", projects(n)));
    }
    files.push(format!("{}/0193-session/tool-results/r.txt", projects(0)));
    files.push(format!("{}/0193-session/subagents/a.jsonl", projects(0)));
    for file in &files {
        let path = data.join(file);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, b"x").unwrap();
    }
    let run = data.join("run").join(OWN_RUN);
    let notes = data.join("agent-state").join(matter_id(0)).join("notes");
    let own = [run.clone(), notes.clone(), home.join("plans")];
    // The project of matter 0's folder, as `claude_projects` finds it for a Windows path.
    let project = vec![data.join(projects(0))];
    assert_eq!(
        claude_projects(&home, &[Path::new(&matter_folder(0))]),
        project
    );
    let deny = launch_deny(vec![data.clone()], &data, &own, &project);
    Big {
        data,
        deny,
        run,
        notes,
    }
}
