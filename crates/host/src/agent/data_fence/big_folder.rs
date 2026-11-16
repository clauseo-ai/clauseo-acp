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
    let mut files: Vec<String> = COMMON.iter().map(|path| (*path).to_owned()).collect();
    for n in [1, 2, 3, 4] {
        let name = if n == 1 {
            PREFIX_RUN.to_owned()
        } else {
            format!("01a1d5c4-90d5-7548-96d0-4a161074492{n}-{n}")
        };
        files.push(format!("run/{name}/work/draft.md"));
        files.push(format!("run/{name}/relay-token"));
    }
    big_rest(data, files, matters, project_count)
}

/// What every big folder holds besides its runs, matters and projects.
const COMMON: [&str; 23] = [
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
];

/// The rest of [`big_with`]: the own run, the matters and projects, written and enumerated.
fn big_rest(data: PathBuf, mut files: Vec<String>, matters: usize, project_count: usize) -> Big {
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

/// The laptop's names (the Windows VM's 60-matter run): the own matter's `UUIDv7` id, its run
/// folder `<v7>-<n>`, its Claude project for `C:\Users\<user>\Documents\Launch Matter`.
pub(crate) const LAPTOP_MATTER: &str = "01a1d670-e53d-7665-ad1a-d33922e44a6f";
pub(crate) const LAPTOP_RUN: &str = "01a1d673-4d5a-776d-9061-201035c7d75e-1";

/// A data folder like the laptop's, under `root`, for the user `user`: the own matter, run and
/// project (`LAPTOP_*`), `matters` seeded matters with random v4 ids (`[guid]::NewGuid()`,
/// from `seed`) and a project each (`C--Users-<user>-Documents-Matters-Seeded-Matter-NN`),
/// two other runs, and what every big folder holds. Returns the launch's rules (with its own work
/// and notes folders, `LaunchDeny::own_reads`), the own project's key and the seeded ids.
pub(crate) fn laptop(
    root: &Path,
    seed: u64,
    matters: usize,
    user: &str,
) -> (LaunchDeny, String, Vec<String>) {
    let data = root.join("data");
    let mut files: Vec<String> = COMMON.iter().map(|path| (*path).to_owned()).collect();
    for run in [
        "01a1d672-1c0e-7d3b-8e11-5a2b9c0d4e6f-2",
        "01a1d673-4d5a-776d-9061-201035c7d75e-2",
    ] {
        files.push(format!("run/{run}/work/x.md"));
    }
    files.push(format!("run/{LAPTOP_RUN}/work/memo.md"));
    files.push(format!("agent-state/{LAPTOP_MATTER}/notes/Notes.md"));
    let mut state = seed;
    let mut guid = || {
        let mut bytes = [0u8; 16];
        for byte in &mut bytes {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            *byte = u8::try_from(state >> 56).unwrap();
        }
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex: String = bytes.iter().fold(String::new(), |mut hex, byte| {
            use std::fmt::Write;
            write!(hex, "{byte:02x}").unwrap();
            hex
        });
        let part = |range: std::ops::Range<usize>| hex.get(range).unwrap().to_owned();
        format!(
            "{}-{}-{}-{}-{}",
            part(0..8),
            part(8..12),
            part(12..16),
            part(16..20),
            part(20..32)
        )
    };
    let ids: Vec<String> = (0..matters).map(|_| guid()).collect();
    let home = data.join("agents/claude-acp/home");
    let own_key = super::claude_project_key(&format!(r"C:\Users\{user}\Documents\Launch Matter"));
    files.push(format!(
        "agents/claude-acp/home/projects/{own_key}/f55bd5d8-b615-4538-ad99-18a52b223f0b.jsonl"
    ));
    for (n, id) in ids.iter().enumerate() {
        files.push(format!("agent-state/{id}/notes/n.md"));
        let key = super::claude_project_key(&format!(
            r"C:\Users\{user}\Documents\Matters\Seeded Matter {:02}",
            n + 1
        ));
        files.push(format!("agents/claude-acp/home/projects/{key}/s.jsonl"));
    }
    for file in &files {
        let path = data.join(file);
        write::create_dir_all(path.parent().unwrap()).unwrap();
        write::write_file(&path, b"x").unwrap();
    }
    let own = [
        data.join("run").join(LAPTOP_RUN),
        data.join("agent-state").join(LAPTOP_MATTER).join("notes"),
        home.join("plans"),
    ];
    let project = vec![home.join("projects").join(&own_key)];
    let mut deny = launch_deny(vec![data.clone()], &data, &own, &project);
    let work = data.join("run").join(LAPTOP_RUN).join("work");
    deny.own_reads = super::own_reads(&data, &[&work, &own[1]]);
    (deny, own_key, ids)
}
