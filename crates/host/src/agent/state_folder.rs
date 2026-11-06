//! Each matter's state folder for each agent with a kind (R17 PR-C):
//! `<data>/agent-state/<matter id>/<agent id>/`, beside the matter's `notes` (R22). Claude Code
//! gets it as `CLAUDE_CONFIG_DIR`, Codex as `CODEX_HOME` (and `HOME` on macOS and Linux), so
//! one matter's transcripts, todos, shell snapshots and `.claude.json` never reach another
//! matter's session. The login homes (`Locations::agent_home`, `<data>/agents/<id>/home`) stay
//! where the login lives: Claude reads its store there through
//! `CLAUDE_SECURESTORAGE_CONFIG_DIR` (or the lawyer's default store, `""`, while reused),
//! and each Codex state folder holds `auth.json` as a hard link to the login home's.
//!
//! Made at a matter's first launch of the agent and kept, on the launch-prep thread
//! ([`make`]: one folder at a time below `<data>`, a link anywhere below it refused and never
//! followed). Before every launch everything in it that the agent does not keep across launches
//! is removed ([`clean`]: an inverted list, [`CODEX_KEPT_FILES`] and [`CLAUDE_KEPT_FILES`]), so
//! nothing a session planted there (instructions, hooks, skills, a `.env`, shell start-up
//! files, auto-memory) reaches the matter's later sessions; Claude's `.claude.json` loses its
//! MCP and tool keys ([`strip_claude_json`]). Clauseo's own files there (`settings.json`,
//! `config.toml`) are written again after the clean. Transcripts are kept, so `session/load`
//! finds them. Files the agent can plant are read only through [`safe_read`]: never through a
//! link, never a file with a second name. The jail's write-deny is R17 PR-D's.
//!
//! Codex's link ([`link_own`], [`link_reused`]): `std::fs::hard_link`, never a symbolic link
//! (Seatbelt checks a symbolic link at its target, decision record 0038), never a copy (a copy
//! would split Codex's rotating refresh token). A link across volumes refuses the launch with
//! `signin.reuse_other_volume`.

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::{AgentKind, ErrorCode, MatterId};

use crate::error::Error;
use crate::log_file::Cause;

mod file_id;
pub(crate) mod safe_read;

pub(crate) use file_id::same_file;

/// The folder under `<data>` that holds every matter's state folders and notes.
pub(crate) const STATE_ROOT: &str = "agent-state";

/// Codex's login file in its home and in each state folder.
pub(crate) const LOGIN_FILE: &str = "auth.json";

/// The temporary name a re-link is made at before it is renamed over [`LOGIN_FILE`], so a
/// Codex already running in the matter never finds the name missing.
const RELINK_NAME: &str = "auth.json.clauseo-link";

/// What Codex's state folder keeps across launches; every other entry is removed before each
/// launch ([`clean`]), whatever its name, so nothing a session planted there (`.env`, which
/// Codex loads at start and exports, `AGENTS.md`, `hooks.json`, `rules`, `skills`, `agents`
/// role files, and with `HOME` the state folder, `.gitconfig`, `.config/git/config`, `.profile`
/// and the rest) reaches the matter's later sessions. Each kept name is Codex's own, written by
/// Codex's process (unsandboxed) or by Clauseo:
/// - `auth.json`: the login link, checked or made again at each launch;
/// - `config.toml`: Clauseo's, written whole after the clean;
/// - `sessions`, `archived_sessions`: the rollouts `session/load` resumes;
/// - `history.jsonl`: the prompt history;
/// - `log`: Codex's own log folder;
/// - `tmp`: the folder Codex's `arg0` links live in for its shell (`codex-rs/arg0`), the one
///   folder of the state folder Codex's shell may write (`agent/codex_fence.rs`);
///
/// and Codex's databases, [`CODEX_DATABASES`] with [`SQLITE_SUFFIXES`].
/// A kept name of the wrong kind (a link, or a folder where a file belongs) is removed too.
pub(crate) const CODEX_KEPT_FILES: [&str; 3] = ["auth.json", "config.toml", "history.jsonl"];

/// The folders of [`CODEX_KEPT_FILES`]' list.
pub(crate) const CODEX_KEPT_FOLDERS: [&str; 4] = ["sessions", "archived_sessions", "log", "tmp"];

/// A SQLite database's own file and its companions.
pub(crate) const SQLITE_SUFFIXES: [&str; 4] = ["", "-wal", "-shm", "-journal"];

/// Codex's databases in its home (`codex-rs/state/src/sqlite.rs:34-39`): thread metadata with
/// each thread's first message (`codex-rs/state/src/extract.rs:186`), logs, goals, memories, the
/// queue and the thread history.
pub(crate) const CODEX_DATABASES: [&str; 6] = [
    "state_5.sqlite",
    "logs_2.sqlite",
    "goals_1.sqlite",
    "memories_1.sqlite",
    "queue_1.sqlite",
    "thread_history_1.sqlite",
];

/// The folder Codex's shell may write in its state folder ([`CODEX_KEPT_FOLDERS`]).
pub(crate) const CODEX_SHELL_FOLDER: &str = "tmp";

/// What Claude Code's state folder keeps across launches; every other entry is removed before
/// each launch ([`clean`]): `CLAUDE.md`, `rules`, `skills`, `plugins`, `agents`, `commands`,
/// `output-styles`, `hooks`, `settings.local.json`, the auto-memory (`projects/*/memory`, an
/// instruction surface: a matter's lasting notes are R22's `notes/` folder), `todos`,
/// `statsig`, `shell-snapshots`, `plans`, `file-history`, `history.jsonl`, a session's
/// `tool-results` and subagent transcripts. Kept:
/// - `.claude.json`: onboarding, the trust list and the account's profile, which Claude Code
///   refetches; its MCP and tool keys are taken out first ([`CLAUDE_JSON_STRIPPED`]), and one
///   that is not JSON, is a link or has a second name is removed;
/// - `settings.json`: Clauseo's, written whole after the clean;
/// - `projects/<key>/<session>.jsonl`: the transcripts `session/load` resumes, nothing else in
///   `projects`.
pub(crate) const CLAUDE_KEPT_FILES: [&str; 2] = [CLAUDE_JSON, "settings.json"];

/// Claude Code's own state file in its config folder.
pub(crate) const CLAUDE_JSON: &str = ".claude.json";

/// The keys of `.claude.json` taken out before each launch, at the top and in each
/// `projects.<path>` entry: user and project MCP servers, the project `.mcp.json` approvals, and
/// the tools allowed without asking. A session could plant any of them to run at every later
/// launch in the matter.
pub(crate) const CLAUDE_JSON_STRIPPED: [&str; 4] = [
    "mcpServers",
    "enabledMcpjsonServers",
    "disabledMcpjsonServers",
    "allowedTools",
];

/// The folders of `state` (`kind`'s state folder for the matter) the host's auto-deny counts
/// as the session's own (`data_fence::FenceCheck::allowed`): for Codex its `tmp`, the one
/// folder its shell reaches; for Claude its `plans` (the plan note) and `projects` (its tool
/// results, this matter's only).
pub(crate) fn fence_own(state: &Path, kind: AgentKind) -> Vec<PathBuf> {
    match kind {
        AgentKind::Codex => vec![state.join(CODEX_SHELL_FOLDER)],
        AgentKind::Claude => vec![state.join("plans"), state.join("projects")],
    }
}

/// What of `state` the auto-deny refuses even where an own folder holds it
/// (`data_fence::FenceCheck::denied`), checked first: the login link and Clauseo's own files,
/// Codex's databases and `.env`, Claude's login store and `.claude.json`.
pub(crate) fn fence_denied(state: &Path, kind: AgentKind) -> Vec<PathBuf> {
    match kind {
        AgentKind::Codex => CODEX_KEPT_FILES
            .iter()
            .copied()
            .chain([".env"])
            .map(|name| state.join(name))
            .chain(CODEX_DATABASES.iter().flat_map(|base| {
                SQLITE_SUFFIXES
                    .iter()
                    .map(move |suffix| state.join(format!("{base}{suffix}")))
            }))
            .collect(),
        AgentKind::Claude => [".credentials.json", CLAUDE_JSON, "settings.json"]
            .iter()
            .map(|name| state.join(name))
            .collect(),
    }
}

/// `<data>/agent-state`.
pub(crate) fn state_root(data: &Path) -> PathBuf {
    data.join(STATE_ROOT)
}

/// `<data>/agent-state/<matter>/<agent>`: the matter's state folder for `agent`.
pub(crate) fn state_dir(data: &Path, matter: MatterId, agent: &str) -> PathBuf {
    state_root(data).join(matter.to_string()).join(agent)
}

/// Makes `state` ([`state_dir`]) one folder at a time below `data`, each private to the user on
/// Unix, a link anywhere below `data` refused and never followed (as the login homes,
/// `codex::make_agent_home`). `who` names the agent in the failure. File system calls: off the
/// actor.
pub(crate) fn make(data: &Path, state: &Path, who: &str) -> Result<(), Error> {
    super::codex::make_agent_home(data, state, who)
}

/// Removes `path`, of `kind` as `symlink_metadata` or `DirEntry::file_type` read it, through no
/// link: a real folder with all it holds (`remove_dir_all`, which removes a link inside and never
/// follows it), anything else (a file, a link, a junction) as itself. A missing name is fine.
fn remove_entry(path: &Path, kind: std::fs::FileType) -> io::Result<()> {
    let removed = if kind.is_dir() {
        crate::write::remove_tree(path)
    } else {
        crate::write::remove_link(path)
    };
    match removed {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Whether Codex keeps the entry `name` of `kind` ([`CODEX_KEPT_FILES`]).
fn codex_keeps(name: &str, kind: std::fs::FileType) -> bool {
    let database = CODEX_DATABASES.iter().any(|base| {
        SQLITE_SUFFIXES
            .iter()
            .any(|suffix| name.strip_prefix(base) == Some(suffix))
    });
    if kind.is_file() {
        CODEX_KEPT_FILES.contains(&name) || database
    } else {
        kind.is_dir() && CODEX_KEPT_FOLDERS.contains(&name)
    }
}

/// Removes every entry of `state` that `kind`'s agent does not keep ([`CODEX_KEPT_FILES`],
/// [`CLAUDE_KEPT_FILES`]), through no link ([`remove_entry`]); for Claude, in `projects` only
/// each project folder's `*.jsonl` files stay. A name that is not UTF-8 is removed. The first
/// failure fails the clean, and so the launch. File system calls: off the actor.
pub(crate) fn clean(state: &Path, kind: AgentKind) -> io::Result<()> {
    for entry in std::fs::read_dir(state)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        let kept = match kind {
            AgentKind::Codex => codex_keeps(name, file_type),
            AgentKind::Claude if name == "projects" && file_type.is_dir() => {
                clean_claude_projects(&entry.path())?;
                true
            }
            AgentKind::Claude => file_type.is_file() && CLAUDE_KEPT_FILES.contains(&name),
        };
        if !kept {
            remove_entry(&entry.path(), file_type)?;
        }
    }
    Ok(())
}

/// In Claude's `projects`, keeps each real project folder's `*.jsonl` plain files and removes
/// everything else, through no link.
fn clean_claude_projects(projects: &Path) -> io::Result<()> {
    for project in std::fs::read_dir(projects)? {
        let project = project?;
        let kind = project.file_type()?;
        if !kind.is_dir() {
            remove_entry(&project.path(), kind)?;
            continue;
        }
        for item in std::fs::read_dir(project.path())? {
            let item = item?;
            let kind = item.file_type()?;
            let name = item.file_name();
            let transcript = kind.is_file()
                && Path::new(&name)
                    .extension()
                    .is_some_and(|extension| extension == "jsonl");
            if !transcript {
                remove_entry(&item.path(), kind)?;
            }
        }
    }
    Ok(())
}

/// Takes [`CLAUDE_JSON_STRIPPED`] out of `<state>/.claude.json`, at the top and in each
/// `projects.<path>` entry, writing it again atomically only when one was there. Read with
/// `safe_read::read_plain`: a link, a file with a second name (a hard link to a credential
/// would be one) or one too large is removed unread, and so is one that is not a JSON object;
/// Claude Code makes a new one. File system calls: off the actor.
pub(crate) fn strip_claude_json(state: &Path) -> io::Result<()> {
    let path = state.join(CLAUDE_JSON);
    let bytes = match safe_read::read_plain(&path)? {
        safe_read::Found::Missing => return Ok(()),
        safe_read::Found::Refused => {
            tracing::warn!("a Claude state file was a link or had a second name; it is removed");
            return remove_name(&path);
        }
        safe_read::Found::Bytes(bytes) => bytes,
    };
    let Ok(serde_json::Value::Object(mut state_json)) = serde_json::from_slice(&bytes) else {
        tracing::warn!("a Claude state file was not a JSON object; it is removed");
        return remove_name(&path);
    };
    let mut stripped = strip_keys(&mut state_json);
    if let Some(serde_json::Value::Object(projects)) = state_json.get_mut("projects") {
        for project in projects.values_mut() {
            if let serde_json::Value::Object(project) = project {
                stripped |= strip_keys(project);
            }
        }
    }
    if !stripped {
        return Ok(());
    }
    let bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(state_json))?;
    crate::write::replace_private_file(&path, &bytes)
}

/// Removes [`CLAUDE_JSON_STRIPPED`] from `object`; whether any was there.
fn strip_keys(object: &mut serde_json::Map<String, serde_json::Value>) -> bool {
    let mut stripped = false;
    for key in CLAUDE_JSON_STRIPPED {
        stripped |= object.remove(key).is_some();
    }
    stripped
}

/// Removes the name `path` itself (never what it points to); a missing one is fine.
fn remove_name(path: &Path) -> io::Result<()> {
    match crate::write::remove_link(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Makes and cleans `kind`'s state folder `state` under `data`: [`make`], then [`clean`]; for
/// Codex its `tmp` made, for Claude its `.claude.json` stripped ([`strip_claude_json`]). The
/// caller writes Clauseo's own file there next.
pub(crate) fn prepare(data: &Path, state: &Path, kind: AgentKind) -> Result<(), Error> {
    let who = match kind {
        AgentKind::Claude => "Claude Code",
        AgentKind::Codex => "Codex",
    };
    make(data, state, who)?;
    let cleaned = clean(state, kind).and_then(|()| match kind {
        AgentKind::Codex => {
            crate::write::create_private_dirs_under(data, &state.join(CODEX_SHELL_FOLDER))
        }
        AgentKind::Claude => strip_claude_json(state),
    });
    cleaned.map_err(|error| {
        tracing::warn!(who, error = %Cause(&error), "an agent's state folder was not cleaned");
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("Clauseo couldn't prepare {who}'s folder."),
        )
    })
}

/// Removes the state folder's `auth.json` name, never what it is linked to: a link the old
/// login left (`turn/sign_in.rs` marks it stale). A missing one is fine.
pub(crate) fn remove_stale_link(state: &Path) -> io::Result<()> {
    remove_name(&state.join(LOGIN_FILE))
}

/// What a Codex launch found of its state folder's `auth.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateLink {
    /// It was already the login home's file.
    Same,
    /// It is now the login home's file: made, or made again.
    Linked,
    /// The login home has no `auth.json`: the state folder has none either (a stale one was
    /// removed, on Clauseo's own login only).
    NoLogin,
    /// While reused: it is there and not the login home's file, or not a plain file. Nothing
    /// was changed.
    Split,
}

/// A link that could not be made.
#[derive(Debug)]
pub(crate) enum LinkError {
    /// The two names are on different volumes: nothing was copied.
    OtherVolume,
    /// Any other failure, its io error kind for the log.
    Failed(io::ErrorKind),
}

impl LinkError {
    fn of(error: &io::Error) -> Self {
        if is_other_volume(error) {
            Self::OtherVolume
        } else {
            Self::Failed(error.kind())
        }
    }

    /// The launch's refusal: `signin.reuse_other_volume` for a link across volumes, else
    /// `agent.launch_failed`. The message names no path.
    pub(crate) fn refusal(&self) -> Error {
        match self {
            Self::OtherVolume => {
                tracing::warn!("a Codex login link would cross volumes; the launch is refused");
                Error::refused(
                    ErrorCode::SigninReuseOtherVolume,
                    "Clauseo can't reuse your Codex login because its data folder is on a \
                     different disk.",
                )
            }
            Self::Failed(kind) => {
                tracing::warn!(error_kind = ?kind, "a matter's Codex login link was not made");
                Error::refused(
                    ErrorCode::AgentLaunchFailed,
                    "Clauseo couldn't prepare Codex's folder.",
                )
            }
        }
    }
}

/// Whether `error` is a link across volumes: `EXDEV` on Unix, `ERROR_NOT_SAME_DEVICE` (17) on
/// Windows, both `CrossesDevices` in std.
pub(crate) fn is_other_volume(error: &io::Error) -> bool {
    const ERROR_NOT_SAME_DEVICE: i32 = 17;
    error.kind() == io::ErrorKind::CrossesDevices
        || (cfg!(windows) && error.raw_os_error() == Some(ERROR_NOT_SAME_DEVICE))
}

/// Whether `path` is a plain file (no folder, link or other reparse point); `None` when it is
/// not there.
fn plain_file(path: &Path) -> io::Result<Option<bool>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta.file_type().is_file() && !reparse_point(&meta))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    /// `FILE_ATTRIBUTE_REPARSE_POINT`.
    const REPARSE: u32 = 0x400;
    meta.file_attributes() & REPARSE != 0
}

#[cfg(not(windows))]
fn reparse_point(_meta: &std::fs::Metadata) -> bool {
    false
}

/// On Clauseo's own login: makes `state`'s `auth.json` the login home's (`home`), comparing
/// file ids first. Already one file: nothing is done. Otherwise a hard link to the home's file
/// is made at a temporary name and renamed over the state folder's, so a Codex already running
/// in the matter never finds it missing. The home with no `auth.json` (signed out, or a key
/// login) removes the state folder's, so a matter never keeps a login the home dropped; a home
/// `auth.json` that is a link or not a plain file is refused, never linked. File system calls:
/// off the actor.
pub(crate) fn link_own(home: &Path, state: &Path) -> Result<StateLink, LinkError> {
    let source = home.join(LOGIN_FILE);
    let target = state.join(LOGIN_FILE);
    match plain_file(&source).map_err(|error| LinkError::of(&error))? {
        None => {
            return match crate::write::remove_link(&target) {
                Ok(()) => Ok(StateLink::NoLogin),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(StateLink::NoLogin),
                Err(error) => Err(LinkError::of(&error)),
            };
        }
        Some(false) => return Err(LinkError::Failed(io::ErrorKind::InvalidInput)),
        Some(true) => {}
    }
    if same_file(&source, &target).unwrap_or(false) {
        return Ok(StateLink::Same);
    }
    let temp = state.join(RELINK_NAME);
    match crate::write::remove_link(&temp) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(LinkError::of(&error));
        }
        _ => {}
    }
    crate::write::hard_link(&source, &temp).map_err(|error| LinkError::of(&error))?;
    if let Err(error) = crate::write::rename_file(&temp, &target) {
        drop(crate::write::remove_link(&temp));
        return Err(LinkError::of(&error));
    }
    Ok(StateLink::Linked)
}

/// While desktop says the login is reused, after the sign-in hook found the login home's
/// `auth.json` to be the lawyer's own (`LoginLink::Shared`): a state folder with no `auth.json`
/// gets a hard link to the home's (so to the lawyer's file, the same one); one that has an
/// `auth.json` that is not the home's file is [`StateLink::Split`] and is never re-linked
/// (R20's split check: the launch is refused and reuse stops). A home with no plain
/// `auth.json` is a split too. Linking from the home's name, not `~/.codex/auth.json`, gives
/// the same file while the hook says the two are one, and keeps the host from naming the
/// lawyer's folder. File system calls: off the actor.
pub(crate) fn link_reused(home: &Path, state: &Path) -> Result<StateLink, LinkError> {
    let source = home.join(LOGIN_FILE);
    let target = state.join(LOGIN_FILE);
    if plain_file(&source).map_err(|error| LinkError::of(&error))? != Some(true) {
        return Ok(StateLink::Split);
    }
    match plain_file(&target).map_err(|error| LinkError::of(&error))? {
        None => {
            crate::write::hard_link(&source, &target).map_err(|error| LinkError::of(&error))?;
            Ok(StateLink::Linked)
        }
        Some(true) if same_file(&source, &target).unwrap_or(false) => Ok(StateLink::Same),
        Some(_) => Ok(StateLink::Split),
    }
}

/// Every matter's state folder for `agent` under `data` that is a real folder: the folders
/// whose `auth.json` `HostHandle::remove_login_links` removes, less the matters in `skip`
/// (named by their folder, the matter id). `agent-state` and each matter's folder must be real
/// folders: a link is skipped, never followed. Only Codex links its login; any other agent has
/// none. A folder that cannot be listed (`agent-state` but a missing one, or an entry) is an
/// error, so the caller never reports links removed that may remain. File system calls: off the
/// actor.
pub(crate) fn linked_folders(
    data: &Path,
    agent: &str,
    skip: &[String],
) -> io::Result<Vec<PathBuf>> {
    if agent != AgentKind::Codex.id() {
        return Ok(Vec::new());
    }
    let root = state_root(data);
    let entries = match std::fs::read_dir(&root) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
        Ok(_) if !crate::write::is_real_dir(&root) => return Ok(Vec::new()),
        Ok(entries) => entries,
    };
    let mut folders = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| skip.iter().any(|m| m == name))
        {
            continue;
        }
        let folder = entry.path().join(agent);
        if crate::write::is_real_dir(&folder) {
            folders.push(folder);
        }
    }
    folders.sort();
    Ok(folders)
}

#[cfg(test)]
mod tests;
