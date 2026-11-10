//! The data-folder fence for Codex: a read and write deny on what of `<data>` only Clauseo uses,
//! and, listed at the launch ([`list`]), on every other session's run folder, every other
//! matter's state, `run.old-*`, and what of `<data>/agents` the shell needs not ([`rules`]), as a
//! Codex permission profile in each Codex session's `CODEX_CONFIG`. The session's own run folder
//! (with the shell browser's `agent-browser` folder in it) and the matter's notes are in no deny,
//! and are `write` entries in the profile and ACP `additionalDirectories`
//! (`actor::turn::profile_loads::codex_directories`).
//!
//! At codex tag `rust-v0.162.1`:
//! - Per-path rules exist only in a named profile: `default_permissions = "<name>"` selects
//!   `[permissions.<name>]`, whose `filesystem` maps a path, or a path and its subpaths, to
//!   `read`, `write` or `deny` (`codex-rs/config/src/permissions_toml.rs:113-118, 224-244`). The
//!   rules are scoped under `<data>` (`{"<data>": {"logs": "deny", ...}}`): a scoped key's base
//!   is read as a literal path, never a glob (`codex-rs/core/src/config/permissions.rs:572-623,
//!   674-705`), so a data folder whose path holds `*`, `?`, `[` or `]` is still named exactly.
//!   A rule may name a path that does not exist yet: bubblewrap masks it once it does, and keeps
//!   a writable root from making it (`codex-rs/linux-sandbox/src/bwrap.rs:1353-1360`); Seatbelt
//!   names it as text.
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
//!   1043-1047`). Seatbelt (macOS) and bubblewrap (Linux) apply a deny to every sandboxed
//!   process, the shell included, for reads and writes (`codex-rs/sandboxing/src/seatbelt.rs:
//!   899-1000`, `codex-rs/linux-sandbox/src/bwrap.rs:1331-1362`).
//! - An approved escalated command (`require_escalated`: agent-browser's, by its prefix rule)
//!   runs under `for_approved_command` (`codex-rs/core/src/tools/orchestrator.rs:234-258`):
//!   `:root` writable and the profile's `deny` entries only, its `write` and `read` entries
//!   dropped (`codex-rs/protocol/src/permissions/target.rs:48-110`, `codex-rs/protocol/src/
//!   permissions.rs:735-766`). So nothing the agent needs may sit under a deny: a deeper entry
//!   reopens it for an ordinary command, never for an escalated one (stage C: every escalated
//!   `agent-browser` failed under `<data>/run` whole and `<data>/browser`). The fence names the
//!   siblings of what it leaves open instead, listed at the launch: other sessions' run folders,
//!   other matters' state folders and the own matter's beside its notes, `run.old-*`, the
//!   entries of `agents` but Codex's own folder and the runtime folder, and those of the runtime
//!   folder but Clauseo's Python builds. A listing that fails denies its folder whole, as before.
//!   An escalated command may write in what no deny names (the Python runtime included): that is
//!   Codex's escalation, which the lawyer approved.
//! - The residual, accepted as for Claude's rules (decision record 0030): a session's run folder,
//!   a matter's state or an entry of `agents` or its runtime made after the launch is in no rule
//!   until the next launch. The sign-in shim, the downloads, the npm cache and settings, Claude's
//!   folder and Codex's login, transcripts and databases are named whether or not they exist, so
//!   no credential is in that residual.
//!
//! Why not a deny of `<data>` whole: bubblewrap reopens only a writable root under a denied
//! folder (`codex-rs/linux-sandbox/src/bwrap.rs:608-660`); read-only roots are bound before the
//! masks and stay hidden (`:536-584`). The shell must read, never write, Codex's own install
//! (its `arg0` links re-run the Codex binary inside the sandbox for `apply_patch`,
//! `codex-rs/arg0/src/lib.rs:329`; its helper paths, `codex-rs/exec-server/src/fs_sandbox.rs:
//! 241-268`), Codex's home, the built-in skills Codex's index names under
//! `<data>/profile/claude-plugin/skills`, the commands the app copies for the agents
//! (`<data>/tools`) and Clauseo's Python runtime (`<data>/agents/runtime/clauseo-python-*`). The
//! run folder is writable whole, so the agent can write this session's pack plugin
//! (`<run>/skill-packs`; decision record 0040 accepts it) and the shell browser's own folder
//! (`<run>/agent-browser`: its config, Chrome profile and downloads). A deny glob is no way to
//! name siblings: bubblewrap expands one with ripgrep at each command, failing past 8192 matches
//! (`codex-rs/linux-sandbox/src/bwrap.rs:891-940`), and Seatbelt's regex for one names only the
//! matching path, not what is under it (`codex-rs/sandboxing/src/seatbelt.rs:719-830`), so
//! [`SET_ASIDE_GLOB`] is kept beside the listed `run.old-*` folders, not instead of them.
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

/// What of Codex's home is denied: its login, its transcripts and its prompt history
/// (`codex-rs/message-history/src/lib.rs:52`), every matter's, with [`CODEX_DATABASES`].
const CODEX_HOME_DENIED: [&str; 4] = [
    "auth.json",
    "sessions",
    "archived_sessions",
    "history.jsonl",
];

/// The folders under `<data>` no Codex shell needs that no `Locations` accessor names: the
/// skill packs' sources, the matters' store, the counts, the Windows sandbox's ledger, the
/// limits reader's state, the browser's config, and the block list's store and the lawyer's
/// allowed sites.
const DATA_DENIED: [&str; 7] = [
    "skill-packs",
    "matters",
    "counts",
    "sandbox",
    "limits",
    "browser",
    "blocklist",
];

/// Under `<data>/agents`, named whether or not they exist: the downloads, npm cache and sign-in
/// shim, and the npm settings files. The rest of `agents` is [`Listed`]'s.
const AGENTS_DENIED: [&str; 5] = ["downloads", "npm-cache", "signin", "npmrc", "npmrc-global"];

/// The run sweep's set-aside folders (`launch_checks/sweep.rs`), as a glob under `<data>`.
pub(crate) const SET_ASIDE_GLOB: &str = "run.old-*";

/// The folders holding every session's run folder and every matter's state, each denied whole
/// when the launch could not list it ([`Listed`]).
const SESSION_PARENTS: [&str; 2] = ["run", "agent-state"];

/// The entries of `<data>/agents` the listing leaves out: Codex's own folder (its install and
/// home, which its shell reads) and the runtime folder (listed itself).
const AGENTS_KEPT: [&str; 2] = ["codex-acp", "runtime"];

/// What the launch listed of the folders the fence names the entries of ([`list`]); each
/// `None` denies its folder whole (the default, before the launch lists anything).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Listed {
    /// Every entry of `<data>/run` but the session's own run folder.
    pub(crate) run: Option<Vec<PathBuf>>,
    /// Every entry of `<data>/agent-state` but this matter's folder, and every entry of that
    /// folder but its notes.
    pub(crate) agent_state: Option<Vec<PathBuf>>,
    /// The `run.old-*` entries of `<data>` (beside [`SET_ASIDE_GLOB`]).
    pub(crate) set_aside: Vec<PathBuf>,
    /// Every entry of `<data>/agents` but [`AGENTS_KEPT`]; none denied whole (Codex's folder is
    /// in it), so `None` adds nothing to [`AGENTS_DENIED`] and Claude's folder.
    pub(crate) agents: Option<Vec<PathBuf>>,
    /// Every entry of `<data>/agents/runtime` but the real folders of a Python build
    /// (`data_fence::is_python_runtime`); `None` when there is none.
    pub(crate) runtime: Option<Vec<PathBuf>>,
}

/// Lists, for [`Listed`], `<data>/run` down to `run_root`, `<data>/agent-state` down to
/// `notes`, the `run.old-*` entries of `data`, `<data>/agents` and `<data>/agents/runtime`.
/// A folder on the way that is not a real folder (a link), a list that fails or a name that is
/// not UTF-8 (no rule could name it) leaves that part `None`. File system calls: off the actor
/// (the launch prep, once the run folder and notes are made).
pub(crate) fn list(data: &Path, run_root: &Path, notes: &Path) -> Listed {
    let run = data.join("run");
    let agent_state = data.join("agent-state");
    let agents = data.join("agents");
    let runtime = agents.join("runtime");
    let python =
        |entry: &Entry| entry.folder && crate::agent::data_fence::is_python_runtime(&entry.name);
    Listed {
        run: down_to(&run, run_root),
        agent_state: down_to(&agent_state, notes),
        set_aside: entries(data)
            .map(|all| {
                all.into_iter()
                    .filter(|entry| entry.name.starts_with("run.old-"))
                    .map(|entry| entry.path)
                    .collect()
            })
            .unwrap_or_default(),
        agents: entries(&agents).map(|all| {
            all.into_iter()
                .filter(|entry| !(entry.folder && AGENTS_KEPT.contains(&entry.name.as_str())))
                .map(|entry| entry.path)
                .collect()
        }),
        runtime: entries(&runtime).and_then(|all| {
            let found = all.iter().any(python);
            found.then(|| {
                all.into_iter()
                    .filter(|entry| !python(entry))
                    .map(|entry| entry.path)
                    .collect()
            })
        }),
    }
}

/// An entry of a listed folder.
struct Entry {
    path: PathBuf,
    name: String,
    /// A real folder, never a link.
    folder: bool,
}

/// The entries of `folder`, sorted; `None` when it is not a real folder, can't be listed, or
/// holds a name that is not UTF-8.
fn entries(folder: &Path) -> Option<Vec<Entry>> {
    if !std::fs::symlink_metadata(folder).is_ok_and(|meta| meta.is_dir()) {
        return None;
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(folder).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().to_str()?.to_owned();
        out.push(Entry {
            path: folder.join(&name),
            name,
            folder: entry.file_type().is_ok_and(|kind| kind.is_dir()),
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Some(out)
}

/// Every entry of `parent` and of each folder on the way down to `own` that is not on the way,
/// so that `own` alone is left; `None` when `own` is not under `parent` by names, or a folder on
/// the way (`own` included) is not a real folder or can't be listed.
fn down_to(parent: &Path, own: &Path) -> Option<Vec<PathBuf>> {
    let names: Vec<String> = own
        .strip_prefix(parent)
        .ok()?
        .components()
        .map(|part| match part {
            std::path::Component::Normal(name) => name.to_str().map(str::to_owned),
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir
            | std::path::Component::ParentDir => None,
        })
        .collect::<Option<_>>()?;
    if names.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    let mut at = parent.to_path_buf();
    for name in names {
        let all = entries(&at)?;
        let next = all.iter().find(|entry| entry.name == name)?;
        if !next.folder {
            return None;
        }
        out.extend(
            all.iter()
                .filter(|entry| entry.name != name)
                .map(|entry| entry.path.clone()),
        );
        at = at.join(name);
    }
    Some(out)
}

/// The session's folders the fence needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Session<'a> {
    /// `<data>/run/<session id>`, a `write` entry.
    pub(crate) run_root: &'a Path,
    /// `<data>/agent-state/<matter>/notes`, a `write` entry.
    pub(crate) notes: &'a Path,
    /// Codex's home, never denied whole.
    pub(crate) codex_home: &'a Path,
    /// What the launch listed ([`list`]).
    pub(crate) listed: &'a Listed,
}

/// The fence's denied paths: the `Locations` paths, the store's and the search index's database
/// files, [`DATA_DENIED`], Claude's whole folder, [`AGENTS_DENIED`], Codex's sign-in markers and
/// [`CODEX_HOME_DENIED`] and [`CODEX_DATABASES`] in its home, then what the launch listed
/// ([`Listed`], each path only under the folder it was listed from). A path that is or holds the
/// session's run folder, its notes or Codex's home is left out, with a warning. Last, each of
/// [`SESSION_PARENTS`] and the runtime folder that the launch could not list, whole: then the
/// session's two folders are reopened by [`keys`]' `write` entries and the session's
/// `additionalDirectories` for an ordinary command only, never an escalated one.
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
    let codex_home = locations.agent_home("codex-acp");
    if let Some(codex) = codex_home.parent() {
        denied.extend(["login-source", "no-reuse"].map(|name| codex.join(name)));
    }
    denied.extend(CODEX_HOME_DENIED.iter().map(|name| codex_home.join(name)));
    for database in CODEX_DATABASES {
        for suffix in SQLITE_SUFFIXES {
            denied.push(codex_home.join(format!("{database}{suffix}")));
        }
    }
    let listed = session.listed;
    let runtime = agents.join("runtime");
    let mut whole = Vec::new();
    let [run, agent_state] = SESSION_PARENTS.map(|name| data.join(name));
    let parents = [
        (run, listed.run.as_ref()),
        (agent_state, listed.agent_state.as_ref()),
        (runtime, listed.runtime.as_ref()),
    ];
    for (parent, entries) in parents {
        match entries {
            Some(entries) => denied.extend(
                entries
                    .iter()
                    .filter(|entry| entry.starts_with(&parent) && **entry != parent)
                    .cloned(),
            ),
            None => whole.push(parent),
        }
    }
    denied.extend(
        listed
            .set_aside
            .iter()
            .filter(|entry| entry.parent() == Some(data))
            .cloned(),
    );
    if let Some(entries) = &listed.agents {
        denied.extend(
            entries
                .iter()
                .filter(|entry| entry.parent() == Some(agents.as_path()))
                .cloned(),
        );
    }
    denied.retain(|path| {
        let own = [session.run_root, session.notes, session.codex_home];
        let keep = !own.iter().any(|folder| folder.starts_with(path));
        if !keep {
            tracing::warn!("a data-folder deny holds a session folder and is left out");
        }
        keep
    });
    denied.extend(whole);
    let mut seen = std::collections::HashSet::new();
    denied.retain(|path| seen.insert(path.clone()));
    denied
}

/// The `CODEX_CONFIG` keys of the fence: `default_permissions` naming [`PROFILE`], and
/// `permissions.<PROFILE>` extending [`EXTENDS`] with each of [`rules`] as `deny` and the
/// session's run folder and notes as `write`, scoped under `<data>`, plus [`SET_ASIDE_GLOB`]
/// when the data folder's path holds no glob character (a scoped glob reads its base as glob
/// text; decision record 0030 names the gap). A rule outside
/// `<data>` is keyed by its own path. Empty on Windows (`windows`). A path that is not UTF-8 is
/// left out with a warning; the launch already fails on a run folder or notes that are not
/// (`Folders::of`), and those sit in the data folder.
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
#[path = "codex_fence/listed_tests.rs"]
mod listed_tests;
#[cfg(test)]
#[path = "codex_fence/tests.rs"]
pub(crate) mod tests;
