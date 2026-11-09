//! The fence proof's meta (R17 PR-D2d): dumps the `_meta` Clauseo really sends in `session/new`
//! for a jailed, non-forced-ask Claude launch, so `tests/live/claude_fences/run.mjs` can replay it
//! against the pinned Claude Code and show every fence holds in `auto`, `acceptEdits` and
//! `default` (decision record 0051, "R17 PR-D2d"). Ignored, since it needs the pinned npm
//! packages and node, which CI does not have; `run.mjs` runs it as
//!
//! ```text
//! CLAUSEO_FENCE_ROOT=<empty dir> CLAUSEO_FENCE_GUARD=<guard program> \
//! CLAUSEO_FENCE_META_OUT=<file.json> \
//! cargo test -p clauseo-host --lib fence_meta_dump -- --ignored --nocapture
//! ```
//!
//! It makes, under `CLAUSEO_FENCE_ROOT`, a data folder laid out as Clauseo's (`data/`: the
//! session's run folder, the matter's notes, Claude's state folder, a settings file, a log and
//! another session's run folder) and a matter folder (`matter/`), then calls the real
//! [`session_meta`] with the real profile fragment ([`ClaudeOptions::new`], no plugin), the real
//! data-fence rules ([`data_fence::launch_deny`] over that folder) and a [`GuardHook`] running
//! `CLAUSEO_FENCE_GUARD`. It writes `{"meta": …, "inputs": …}` to `CLAUSEO_FENCE_META_OUT`.

use std::path::{Path, PathBuf};

use clauseo_profile::ClaudeOptions;
use serde_json::json;

use super::browser::Switch;
use super::profile::{ClaudeKeys, Folders, Login};
use super::{GuardHook, McpControl, session_meta};
use crate::agent::data_fence;
use crate::browser::seed_sites::SEED_SITES;

const SESSION: &str = "fence-session";
const OTHER_SESSION: &str = "other-session";
const MATTER_ID: &str = "matter-1";

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is not set")))
}

fn mkdir(path: &Path) {
    crate::write::create_dir_all(path).unwrap();
}

#[test]
#[ignore = "D2d harness: dumps the real Claude _meta to CLAUSEO_FENCE_META_OUT"]
fn fence_meta_dump() {
    let root = env_path("CLAUSEO_FENCE_ROOT");
    let guard = env_path("CLAUSEO_FENCE_GUARD");
    let out = env_path("CLAUSEO_FENCE_META_OUT");
    mkdir(&root);
    let root = dunce::canonicalize(&root).unwrap();
    let data = root.join("data");
    let matter = root.join("matter");
    let run_root = data.join("run").join(SESSION);
    let work = run_root.join("work");
    let notes = data.join("agent-state").join(MATTER_ID).join("notes");
    let state = data.join("agent-state").join(MATTER_ID).join("claude-acp");
    for dir in [
        &matter,
        &work,
        &notes,
        &state.join("plans"),
        &data.join("logs"),
        &data.join("run").join(OTHER_SESSION).join("work"),
    ] {
        mkdir(dir);
    }
    for (file, text) in [
        (
            data.join(clauseo_settings::SETTINGS_FILE),
            "{\"secret\": \"data settings\"}\n",
        ),
        (data.join("logs").join("host.log"), "secret log line\n"),
        (
            data.join("run")
                .join(OTHER_SESSION)
                .join("work")
                .join("other.txt"),
            "another session's work\n",
        ),
    ] {
        crate::write::write_file(&file, text.as_bytes()).unwrap();
    }

    // As `launch_checks::folders::launch_deny` does for a Claude launch.
    let cwd = crate::agent::session_folder(&matter);
    let mut spellings: Vec<PathBuf> = dunce::canonicalize(&data).into_iter().collect();
    if !spellings.contains(&data) {
        spellings.push(data.clone());
    }
    let own = vec![run_root, notes.clone(), cwd.clone(), state.join("plans")];
    let projects = data_fence::claude_projects(&state, &[&cwd, &matter]);
    let deny = data_fence::launch_deny(spellings, &data, &own, &projects);

    let folders = Folders::of(&work, &notes).unwrap();
    let keys = ClaudeKeys {
        web_search: Switch {
            on: false,
            locked: false,
        },
        login: Login::FirstParty,
        folders,
        seed_sites: SEED_SITES,
        forced_ask: false,
        data: &deny,
        case_law: false,
    };
    let hook = ClaudeOptions::new("Clauseo fence proof (D2d).".to_owned(), None).meta_fragment();
    let guard_hook = GuardHook {
        program: &guard,
        matter: &matter,
        ask_all: false,
    };
    let mut meta = session_meta(
        Some(hook),
        McpControl::Clauseo,
        Some(guard_hook),
        Some(&keys),
    );
    super::profile::fill_meta(&mut meta, folders).unwrap();

    let dump = json!({
        "meta": meta,
        "inputs": {
            "root": root,
            "data": data,
            "matter": matter,
            "cwd": cwd,
            "work": work,
            "notes": notes,
            "claude_state": state,
            "guard": guard,
            "web_search": {"on": false, "locked": false},
            "login": "FirstParty",
            "forced_ask": false,
            "case_law": false,
            "mcp_control": "Clauseo",
            "hook": "ClaudeOptions::new(\"Clauseo fence proof (D2d).\", None).meta_fragment()",
            "deny_targets": deny.targets,
        },
    });
    crate::write::write_file(
        &out,
        serde_json::to_string_pretty(&dump).unwrap().as_bytes(),
    )
    .unwrap();
    println!("wrote {}", out.display());
}
