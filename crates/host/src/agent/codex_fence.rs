//! The data-folder fence for Codex: a read and write deny on what of `<data>` only Clauseo uses,
//! and on `<data>/run`, `<data>/agent-state` and `<data>/run.old-*` whole ([`rules`]), as a
//! Codex permission profile in each Codex session's `CODEX_CONFIG`. The session's own run folder,
//! the matter's notes and the matter's Codex state folder (its `CODEX_HOME`, R17 PR-C) are
//! reopened as `write` entries in the profile and as ACP `additionalDirectories`
//! (`actor::turn::profile_loads::codex_directories`), so a session or a matter made after the
//! launch is denied too, and so is every other matter's state folder. Inside the own state
//! folder the login link, the transcripts, the databases, `config.toml` and the config surfaces
//! stay denied (deeper rules win); those surfaces are cleaned at each launch
//! (`agent/state_folder.rs`) and their write-deny under the jail is R17 PR-D's.
//!
//! At codex tag `rust-v0.162.1`:
//! - Per-path rules exist only in a named profile: `default_permissions = "<name>"` selects
//!   `[permissions.<name>]`, whose `filesystem` maps a path, or a path and its subpaths, to
//!   `read`, `write` or `deny` (`codex-rs/config/src/permissions_toml.rs:113-118, 224-244`). The
//!   rules are scoped under `<data>` (`{"<data>": {"logs": "deny", ...}}`): a scoped key's base
//!   is read as a literal path, never a glob (`codex-rs/core/src/config/permissions.rs:572-623,
//!   674-705`), so a data folder whose path holds `*`, `?`, `[` or `]` is still named exactly.
//! - A `default_permissions` in the request's config (codex-acp sends `CODEX_CONFIG` as each
//!   thread's `config`, the session-flags layer) picks the profile syntax
//!   (`codex-rs/core/src/config/mod.rs:2594-2615`); the profile extends `:workspace`
//!   (`codex-rs/core/src/config/permissions.rs:213-223`). Under it `CODEX_CONFIG`'s
//!   `sandbox_workspace_write.writable_roots` no longer reaches a turn (stage B, live, macOS).
//! - codex-acp sends its `workspaceWrite` policy with every `turn/start`, its writable roots the
//!   session's `additionalDirectories` (codex-acp 2.2.2 `dist/index.js:35598-35612,
//!   35878-35885`; `codex-rs/app-server/src/request_processors/turn_processor.rs:846, 893, 918`);
//!   Codex rebuilds the policy from it and keeps the profile's deny entries
//!   (`codex-rs/core/src/session/session.rs:491-507`, `codex-rs/protocol/src/permissions.rs:
//!   709-731`).
//! - The deepest matching rule wins and `deny` wins a tie (`codex-rs/protocol/src/permissions.rs:
//!   1043-1048`), so the own run folder and notes, deeper than the `run` and `agent-state`
//!   denies, stay writable. Seatbelt (macOS) and bubblewrap (Linux) apply a deny to every
//!   sandboxed process, the shell included, for reads and writes (`codex-rs/sandboxing/src/
//!   seatbelt.rs:899-900, 1045-1046`, `codex-rs/linux-sandbox/src/bwrap.rs:490-511, 1353-1362`).
//!   bubblewrap masks a denied folder, then binds a writable root under it
//!   (`codex-rs/linux-sandbox/src/bwrap.rs:615-659, 1370-1398`); Seatbelt reads every writable
//!   root (`codex-rs/protocol/src/permissions.rs:1466-1476`) and holds a deny as an excluded
//!   subpath (`codex-rs/sandboxing/src/seatbelt.rs:966-981, 1007-1020`).
//!
//! Why not a deny of `<data>` whole: bubblewrap reopens only a writable root under a denied
//! folder (`codex-rs/linux-sandbox/src/bwrap.rs:608-660`); read-only roots are bound before the
//! masks and stay hidden (`:536-584`). The shell must read, never write, Codex's own install
//! (its `arg0` links re-run the Codex binary inside the sandbox for `apply_patch`,
//! `codex-rs/arg0/src/lib.rs:329`; its helper paths, `codex-rs/exec-server/src/fs_sandbox.rs:
//! 241-268`), Codex's home, and the built-in skills Codex's index names under
//! `<data>/profile/claude-plugin/skills`. The run folder is writable whole, so the agent can
//! write this session's pack plugin (`<run>/skill-packs`; decision record 0040 accepts it).
//!
//! A deny binds only sandboxed processes; Codex's own process writes its login, transcripts and
//! databases in its home unsandboxed (`codex-rs/state/src/sqlite.rs:34-39, 61-63`,
//! `codex-rs/message-history/src/lib.rs:52, 86`).
//!
//! Not on Windows: the unelevated sandbox Clauseo gives Codex there refuses to run any sandboxed
//! command under a read deny (`codex-rs/sandboxing/src/windows.rs:111-131`).

use std::path::{Path, PathBuf};

use clauseo_settings::Locations;
use serde_json::{Map, Value};

/// The profile's name in `[permissions]`.
pub(crate) const PROFILE: &str = "clauseo";
/// The built-in profile it extends: Codex's workspace-write.
pub(crate) const EXTENDS: &str = ":workspace";

/// A SQLite database's own file and its companions.
const SQLITE_SUFFIXES: [&str; 4] = ["", "-wal", "-shm", "-journal"];

/// Codex's databases in its home (`codex-rs/state/src/sqlite.rs:34-39`): thread metadata with
/// each thread's first message (`codex-rs/state/src/extract.rs:186`), logs, goals, memories, the
/// queue and the thread history, every matter's.
const CODEX_DATABASES: [&str; 6] = [
    "state_5.sqlite",
    "logs_2.sqlite",
    "goals_1.sqlite",
    "memories_1.sqlite",
    "queue_1.sqlite",
    "thread_history_1.sqlite",
];

/// What of the session's state folder (its `CODEX_HOME`) is denied: its login link (the live
/// login, every matter's through the one file), its transcripts and its prompt history
/// (`codex-rs/message-history/src/lib.rs:52`), with [`CODEX_DATABASES`], `config.toml` and the
/// config surfaces each launch cleans (`state_folder::CODEX_REMOVED`): Codex itself reads and
/// writes them unsandboxed, and no shell command needs them.
const CODEX_HOME_DENIED: [&str; 5] = [
    "auth.json",
    "sessions",
    "archived_sessions",
    "history.jsonl",
    "config.toml",
];

/// The folders under `<data>` no Codex shell needs that no `Locations` accessor names: the
/// skill packs' sources, the matters' store, the counts, the Windows sandbox's ledger, the
/// limits reader's state and the browser's config.
const DATA_DENIED: [&str; 6] = [
    "skill-packs",
    "matters",
    "counts",
    "sandbox",
    "limits",
    "browser",
];

/// Under `<data>/agents`: the shared runtime, downloads, npm cache and sign-in shim, and the npm
/// settings files.
const AGENTS_DENIED: [&str; 6] = [
    "runtime",
    "downloads",
    "npm-cache",
    "signin",
    "npmrc",
    "npmrc-global",
];

/// The run sweep's set-aside folders (`launch_checks/sweep.rs`), as a glob under `<data>`.
pub(crate) const SET_ASIDE_GLOB: &str = "run.old-*";

/// The folders holding every session's run folder and every matter's notes, denied whole.
const SESSION_PARENTS: [&str; 2] = ["run", "agent-state"];

/// The session's folders the fence needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Session<'a> {
    /// `<data>/run/<session id>`, a `write` entry.
    pub(crate) run_root: &'a Path,
    /// `<data>/agent-state/<matter>/notes`, a `write` entry.
    pub(crate) notes: &'a Path,
    /// The matter's state folder, `<data>/agent-state/<matter>/codex-acp`, Codex's
    /// `CODEX_HOME` (R17 PR-C): a `write` entry, so the shell reaches what Codex keeps there for
    /// it (its `arg0` links under `tmp`), with what [`CODEX_HOME_DENIED`] names in it denied.
    /// Every other matter's state folder stays under the deny of `agent-state`.
    pub(crate) state: &'a Path,
}

/// The fence's denied paths: the `Locations` paths, the store's and the search index's database
/// files, [`DATA_DENIED`], both agents' login homes and Claude's whole folder,
/// [`AGENTS_DENIED`], Codex's sign-in markers, [`CODEX_HOME_DENIED`] and [`CODEX_DATABASES`] in
/// the session's state folder, then [`SESSION_PARENTS`]. A path of the first group that is or
/// holds the session's run folder, its notes or its state folder is left out, with a warning;
/// the session's three folders are reopened under [`SESSION_PARENTS`] by [`keys`]' `write`
/// entries and the session's `additionalDirectories`.
pub(crate) fn rules(locations: &Locations, session: Session<'_>) -> Vec<PathBuf> {
    let data = locations.data_dir();
    let agents = data.join("agents");
    let mut denied: Vec<PathBuf> = vec![
        locations.log_dir(),
        locations.updates_dir(),
        locations.update_marker(),
        locations.settings_file().to_path_buf(),
        data.join(clauseo_counts::FIRST_ACT),
        data.join(clauseo_store::LOCK_FILE),
    ];
    for database in [clauseo_store::DB_FILE, clauseo_search::FILE_NAME] {
        for suffix in SQLITE_SUFFIXES {
            denied.push(data.join(format!("{database}{suffix}")));
        }
    }
    denied.extend(DATA_DENIED.iter().map(|name| data.join(name)));
    // Claude's whole folder: its home (`Locations::agent_home`), install and markers.
    if let Some(claude) = locations.agent_home("claude-acp").parent() {
        denied.push(claude.to_path_buf());
    }
    denied.extend(AGENTS_DENIED.iter().map(|name| agents.join(name)));
    // Codex's login home whole: its own login, and every matter's transcripts from before
    // R17 PR-C. The session's `CODEX_HOME` is its state folder.
    let codex_home = locations.agent_home("codex-acp");
    if let Some(codex) = codex_home.parent() {
        denied.extend(["login-source", "no-reuse"].map(|name| codex.join(name)));
    }
    denied.push(codex_home);
    let state = session.state;
    denied.extend(CODEX_HOME_DENIED.iter().map(|name| state.join(name)));
    for database in CODEX_DATABASES {
        for suffix in SQLITE_SUFFIXES {
            denied.push(state.join(format!("{database}{suffix}")));
        }
    }
    denied.extend(
        crate::agent::state_folder::CODEX_REMOVED
            .iter()
            .map(|name| state.join(name)),
    );
    denied.retain(|path| {
        let own = [session.run_root, session.notes, session.state];
        let keep = !own.iter().any(|folder| folder.starts_with(path));
        if !keep {
            tracing::warn!("a data-folder deny holds a session folder and is left out");
        }
        keep
    });
    denied.extend(SESSION_PARENTS.iter().map(|name| data.join(name)));
    denied
}

/// The `CODEX_CONFIG` keys of the fence: `default_permissions` naming [`PROFILE`], and
/// `permissions.<PROFILE>` extending [`EXTENDS`] with each of [`rules`] as `deny` and the session's
/// run folder, notes and state folder as `write`, scoped under `<data>`, plus [`SET_ASIDE_GLOB`]
/// when the data folder's path holds no glob character (a scoped glob reads its base as glob text;
/// decision record 0030 names the gap). A rule outside `<data>` is keyed by its own path. Empty on
/// Windows (`windows`). A path that is not UTF-8 is left out with a warning; the launch already
/// fails on a run folder or notes that are not (`Folders::of`), and those sit in the data folder.
pub(crate) fn keys(
    locations: &Locations,
    session: Session<'_>,
    windows: bool,
) -> Map<String, Value> {
    let mut keys = Map::new();
    if windows {
        return keys;
    }
    let data = locations.data_dir();
    let Some(base) = data.to_str() else {
        tracing::warn!("the data folder is not UTF-8: Codex gets no data-folder deny");
        return keys;
    };
    let mut scoped = Map::new();
    let mut filesystem = Map::new();
    let denies = rules(locations, session)
        .into_iter()
        .map(|path| (path, "deny"));
    let own =
        [session.run_root, session.notes, session.state].map(|path| (path.to_path_buf(), "write"));
    for (path, mode) in denies.chain(own) {
        match path.strip_prefix(data).ok().and_then(Path::to_str) {
            Some(relative) if !relative.is_empty() => {
                scoped.insert(relative.replace('\\', "/"), Value::from(mode));
            }
            _ => {
                if let Some(text) = path.to_str() {
                    filesystem.insert(text.to_owned(), Value::from(mode));
                } else {
                    tracing::warn!("a data-folder rule is not UTF-8 and is left out");
                }
            }
        }
    }
    if base.contains(['*', '?', '[', ']', '{', '}']) {
        tracing::warn!("the data folder's path holds a glob character: run.old-* stays readable");
    } else {
        scoped.insert(SET_ASIDE_GLOB.to_owned(), Value::from("deny"));
    }
    filesystem.insert(base.to_owned(), Value::Object(scoped));
    let mut profile = Map::new();
    profile.insert("extends".to_owned(), Value::from(EXTENDS));
    profile.insert("filesystem".to_owned(), Value::Object(filesystem));
    let mut permissions = Map::new();
    permissions.insert(PROFILE.to_owned(), Value::Object(profile));
    keys.insert("default_permissions".to_owned(), Value::from(PROFILE));
    keys.insert("permissions".to_owned(), Value::Object(permissions));
    keys
}

#[cfg(test)]
#[path = "codex_fence/tests.rs"]
mod tests;
