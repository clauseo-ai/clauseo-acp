//! The data-folder fence for Codex: a read and write deny on what of `<data>` only Clauseo uses,
//! on `<data>/run` and `<data>/run.old-*` whole, and on `<data>/agent-state` entry by entry
//! ([`rules`], [`state_denies`]), as a Codex permission profile in each Codex session's
//! `CODEX_CONFIG`. The session's own run folder (with its `tmp`, the shell's `TMPDIR`) and the
//! matter's notes are reopened as `write` entries in the profile and as ACP
//! `additionalDirectories` (`actor::turn::profile_loads::codex_directories`), so a session made
//! after the launch is denied too. Every other matter's folder that exists at the launch is
//! denied, and every entry of the own Codex state folder (its `CODEX_HOME`, R17 PR-C) but its
//! `tmp`: the shell never reads the login link and never writes what Codex loads at the next
//! launch, and reads but never writes the `arg0` folder Codex puts first on its `PATH`. A
//! matter made after the launch is the named residual. The state folder is cleaned at each
//! launch to what Codex keeps (`agent/state_folder.rs`); the jail's write-deny is R17 PR-D's.
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

use crate::agent::state_folder::SQLITE_SUFFIXES;

/// The profile's name in `[permissions]`.
pub(crate) const PROFILE: &str = "clauseo";
/// The built-in profile it extends: Codex's workspace-write.
pub(crate) const EXTENDS: &str = ":workspace";

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

/// The folder holding every session's run folder, denied whole. `agent-state` is denied entry
/// by entry ([`state_denies`]), so the own state folder's `tmp` stays readable.
const SESSION_PARENTS: [&str; 1] = ["run"];

/// The names of the own state folder denied even when they do not exist at the launch: Codex's
/// `.env` and every name Codex keeps but `tmp` (`state_folder::CODEX_KEPT_FILES`, the login link
/// and `config.toml` among them, `CODEX_KEPT_FOLDERS`, the databases), so what Codex makes in its
/// first session is covered. For the shell only: Codex's own process is not fenced.
fn state_always_denied() -> Vec<String> {
    use crate::agent::state_folder::{
        CODEX_DATABASES, CODEX_KEPT_FILES, CODEX_KEPT_FOLDERS, CODEX_SHELL_FOLDER,
    };
    let mut names: Vec<String> = [".env"]
        .iter()
        .chain(&CODEX_KEPT_FILES)
        .chain(
            CODEX_KEPT_FOLDERS
                .iter()
                .filter(|name| **name != CODEX_SHELL_FOLDER),
        )
        .map(|name| (*name).to_owned())
        .collect();
    for base in CODEX_DATABASES {
        names.extend(
            SQLITE_SUFFIXES
                .iter()
                .map(|suffix| format!("{base}{suffix}")),
        );
    }
    names
}

/// Deny globs under `<data>`, for every matter, one made after the launch included (round 5):
/// another matter's Codex login link, `.env`, `config.toml`, transcripts, prompt history, log
/// and databases, and every matter's Claude state folder. codex-rs keeps glob entries (deny
/// entries) when it rebuilds each turn's policy (`protocol/src/permissions.rs`
/// `from_legacy_sandbox_policy_preserving_deny_entries`), and bubblewrap expands them at each
/// command (`linux-sandbox/src/bwrap.rs:241-245`), so a matter made mid-session is covered from
/// the next command. A glob entry is deny only, with no exception syntax, so `agent-state/*/**`
/// with the own `tmp` and `notes` left out cannot be written; another matter's `notes` made
/// after the launch stays readable (the named residual), since the own notes must stay
/// writable and a glob deny wins over it.
pub(crate) const STATE_GLOBS: [&str; 9] = [
    "agent-state/*/codex-acp/auth.json",
    "agent-state/*/codex-acp/.env",
    "agent-state/*/codex-acp/config.toml",
    "agent-state/*/codex-acp/sessions/**",
    "agent-state/*/codex-acp/archived_sessions/**",
    "agent-state/*/codex-acp/history.jsonl",
    "agent-state/*/codex-acp/log/**",
    "agent-state/*/codex-acp/*.sqlite*",
    "agent-state/*/claude-acp/**",
];

/// The `agent-state` denies of a Codex launch whose state folder is `state` under `data`, read
/// on the launch-prep thread (round 4's ruling, option B): every other matter's folder that
/// exists, every entry of this matter's folder but its `notes` and `codex-acp`, and every entry
/// of the state folder but `tmp`, plus [`state_always_denied`] by name. So `tmp`, where Codex
/// keeps the `arg0` links it puts first on `PATH` (`apply_patch`, `codex-execve-wrapper`, and the
/// sandbox helper bubblewrap before 0.9 re-runs, codex-rs `linux_run_main.rs:404-435`), stays
/// readable and runnable for the shell, and never writable (no `write` entry, no writable root:
/// round 3's H1). A matter made after the launch, and a name Codex makes in its state folder
/// during the session, are not denied (the residuals). Entries are listed through no link; a
/// folder that cannot be listed is denied whole.
pub(crate) fn state_denies(data: &Path, state: &Path) -> Vec<PathBuf> {
    let root = data.join(crate::agent::state_folder::STATE_ROOT);
    let (Some(matter), Some(agent)) = (state.parent(), state.file_name()) else {
        return vec![root];
    };
    let mut denied = Vec::new();
    let others = |folder: &Path, keep: &[&std::ffi::OsStr], denied: &mut Vec<PathBuf>| {
        match std::fs::read_dir(folder) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    if !keep.contains(&entry.file_name().as_os_str()) {
                        denied.push(entry.path());
                    }
                }
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(_) => false,
        }
    };
    let matter_name = matter.file_name().unwrap_or_default();
    let listed = others(&root, &[matter_name], &mut denied)
        && others(matter, &[std::ffi::OsStr::new("notes"), agent], &mut denied)
        && others(
            state,
            &[std::ffi::OsStr::new(
                crate::agent::state_folder::CODEX_SHELL_FOLDER,
            )],
            &mut denied,
        );
    if !listed {
        tracing::warn!("a state folder could not be listed: agent-state is denied whole");
        return vec![root];
    }
    for name in state_always_denied() {
        let path = state.join(name);
        if !denied.contains(&path) {
            denied.push(path);
        }
    }
    denied
}

/// The session's folders the fence needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Session<'a> {
    /// `<data>/run/<session id>`, a `write` entry.
    pub(crate) run_root: &'a Path,
    /// `<data>/agent-state/<matter>/notes`, a `write` entry.
    pub(crate) notes: &'a Path,
    /// The matter's state folder, `<data>/agent-state/<matter>/codex-acp`, Codex's
    /// `CODEX_HOME` and `HOME` (R17 PR-C). Every entry but `tmp` is denied ([`state_denies`]),
    /// so the shell neither reads its login link, transcripts and databases nor writes anything
    /// Codex loads; `tmp` is readable and never writable, since its `arg0/codex-arg0*` folder
    /// Codex puts first on
    /// `PATH` (codex-rs `arg0/src/lib.rs:357`) and whose `git` Codex's own unsandboxed calls
    /// would run (`git-utils/src/info.rs:375, 708`). The shell's temp folder is the run
    /// folder's `tmp` (`TMPDIR`), under its `write` entry.
    pub(crate) state: &'a Path,
    /// The `agent-state` denies read at the launch ([`state_denies`]).
    pub(crate) state_denies: &'a [PathBuf],
}

/// The fence's denied paths: the `Locations` paths, the store's and the search index's database
/// files, [`DATA_DENIED`], both agents' login homes whole and Claude's whole folder,
/// [`AGENTS_DENIED`], Codex's sign-in markers, then [`SESSION_PARENTS`]. A path of the first
/// group that is or holds the session's run folder, its notes or its state folder is left out,
/// with a warning; then [`SESSION_PARENTS`] and the launch's `agent-state` denies
/// ([`state_denies`]). The session's run folder is reopened under `run` by [`keys`]' `write`
/// entries and the session's `additionalDirectories`, and so are this matter's notes.
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
    denied.retain(|path| {
        let own = [session.run_root, session.notes, session.state];
        let keep = !own.iter().any(|folder| folder.starts_with(path));
        if !keep {
            tracing::warn!("a data-folder deny holds a session folder and is left out");
        }
        keep
    });
    denied.extend(SESSION_PARENTS.iter().map(|name| data.join(name)));
    denied.extend(session.state_denies.iter().cloned());
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
    let own = [session.run_root, session.notes].map(|path| (path.to_path_buf(), "write"));
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
        tracing::warn!(
            "the data folder's path holds a glob character: run.old-* and later matters' \
             state stay readable"
        );
    } else {
        scoped.insert(SET_ASIDE_GLOB.to_owned(), Value::from("deny"));
        for glob in STATE_GLOBS {
            scoped.insert(glob.to_owned(), Value::from("deny"));
        }
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
#[path = "codex_fence/state_tests.rs"]
mod state_tests;
#[cfg(test)]
#[path = "codex_fence/tests.rs"]
mod tests;
