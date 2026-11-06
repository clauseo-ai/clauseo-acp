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

mod claude_json;
mod file_id;
#[cfg(unix)]
mod handle;
#[cfg(not(unix))]
mod projects;
pub(crate) mod safe_read;

pub(crate) use claude_json::strip_claude_json;
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
/// - `cap_sid`: Codex's Windows sandbox keeps its capability SIDs there
///   (`codex-rs/windows-sandbox-rs/src/cap.rs`); a new one at each launch could add new
///   permission entries to the matter folder every time. Codex's own state, no instruction;
/// - `log`: Codex's own log folder;
/// - `tmp`: Codex's own temp folder, where it makes the `arg0` links (`codex-rs/arg0`); denied
///   to Codex's shell (`agent/codex_fence.rs`), whose `TMPDIR` is the run folder's `tmp`;
///
/// and Codex's databases, [`CODEX_DATABASES`] with [`SQLITE_SUFFIXES`].
/// A kept name of the wrong kind (a link, or a folder where a file belongs) is removed too.
pub(crate) const CODEX_KEPT_FILES: [&str; 4] =
    ["auth.json", "config.toml", "history.jsonl", "cap_sid"];

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

/// Codex's own temp folder in its state folder, where it makes the `arg0` links it puts first
/// on `PATH` ([`CODEX_KEPT_FOLDERS`]): written by Codex's process only, never by its shell.
pub(crate) const CODEX_SHELL_FOLDER: &str = "tmp";

/// What Claude Code's state folder keeps across launches; every other entry is removed before
/// each launch ([`clean`]): `CLAUDE.md`, `rules`, `skills`, `plugins`, `agents`, `commands`,
/// `output-styles`, `hooks`, `settings.local.json`, the auto-memory (`projects/*/memory`, an
/// instruction surface: a matter's lasting notes are R22's `notes/` folder), `todos`,
/// `statsig`, `shell-snapshots`, `plans`, `file-history`, `history.jsonl`, a session's
/// `tool-results` and subagent transcripts. Kept:
/// - `.claude.json`: onboarding, the trust list and the account's profile, which Claude Code
///   refetches; cut to an allow-list of keys first (`claude_json::CLAUDE_JSON_KEPT`), and one
///   that is not JSON, is a link or has a second name is removed;
/// - `settings.json`: Clauseo's, written whole after the clean;
/// - `projects/<key>/<session>.jsonl`: the transcripts `session/load` resumes, nothing else in
///   `projects`.
pub(crate) const CLAUDE_KEPT_FILES: [&str; 2] = [CLAUDE_JSON, "settings.json"];

/// Claude Code's own state file in its config folder.
pub(crate) const CLAUDE_JSON: &str = ".claude.json";

/// The folders of `state` (`kind`'s state folder for the matter) the host's auto-deny counts
/// as the session's own (`data_fence::FenceCheck::allowed`): for Codex none (its shell's temp
/// folder is the run folder's; the state folder's `tmp` holds the `arg0` links Codex puts first
/// on `PATH`); for Claude its `plans` (the plan note) and `projects` (its tool results, this
/// matter's only).
pub(crate) fn fence_own(state: &Path, kind: AgentKind) -> Vec<PathBuf> {
    match kind {
        AgentKind::Codex => Vec::new(),
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
            .chain([".env", CODEX_SHELL_FOLDER])
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
#[cfg(not(unix))]
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
fn codex_keeps(name: &str, folder: bool, file: bool) -> bool {
    let database = CODEX_DATABASES.iter().any(|base| {
        SQLITE_SUFFIXES
            .iter()
            .any(|suffix| name.strip_prefix(base) == Some(suffix))
    });
    if file {
        CODEX_KEPT_FILES.contains(&name) || database
    } else {
        folder && CODEX_KEPT_FOLDERS.contains(&name)
    }
}

/// The folders of Claude's state folder a live session reads back or writes while it runs, kept
/// beside a live session ([`clean`] with `live`): its plan note, its edited files' copies, its
/// to-do lists and its shell snapshots. In `projects` the session folders (tool results,
/// subagents) are kept too, never `memory`.
pub(crate) const CLAUDE_LIVE_FOLDERS: [&str; 4] =
    ["plans", "file-history", "todos", "shell-snapshots"];

/// Whether the file `path` has another name: a hard link (to a credential, say) is never kept.
/// Off Unix (Unix reads the link count on the folder's handle, `handle.rs`).
#[cfg(not(unix))]
pub(crate) fn has_second_name(path: &Path) -> io::Result<bool> {
    #[cfg(windows)]
    {
        Ok(file_id::link_count(path)?.is_some_and(|links| links > 1))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(true)
    }
}

/// Removes every entry of `state` that `kind`'s agent does not keep ([`CODEX_KEPT_FILES`],
/// [`CLAUDE_KEPT_FILES`]), through no link ([`remove_entry`]); a kept file with a second name is
/// removed too, but Codex's `auth.json` (the login link, checked by its own rules). For Claude,
/// in `projects` only each project folder's `*.jsonl` files stay (`projects.rs`, by handle).
/// With `live` (another session of the agent runs in the matter) Claude keeps the working data
/// a live session reads back as well ([`CLAUDE_LIVE_FOLDERS`], session folders in `projects`);
/// every instruction surface is removed either way. A name that is not UTF-8 is removed. The
/// first failure fails the clean, and so the launch. File system calls: off the actor.
/// On Unix by handle (`handle.rs`); elsewhere by path, refusing reparse points (`projects.rs`).
pub(crate) fn clean(state: &Path, kind: AgentKind, live: bool) -> io::Result<()> {
    #[cfg(unix)]
    {
        handle::clean(state, kind, live)
    }
    #[cfg(not(unix))]
    {
        clean_by_path(state, kind, live)
    }
}

/// [`clean`] by path, off Unix: each entry's kind from the folder listing (a link or junction
/// is removed as itself), Claude's projects through `projects.rs`.
#[cfg(not(unix))]
fn clean_by_path(state: &Path, kind: AgentKind, live: bool) -> io::Result<()> {
    for entry in std::fs::read_dir(state)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name = name.to_str().unwrap_or_default();
        let kept = match kind {
            AgentKind::Codex => codex_keeps(name, file_type.is_dir(), file_type.is_file()),
            AgentKind::Claude if name == "projects" && file_type.is_dir() => {
                projects::clean(&entry.path(), live)?;
                true
            }
            AgentKind::Claude if file_type.is_dir() => live && CLAUDE_LIVE_FOLDERS.contains(&name),
            AgentKind::Claude => file_type.is_file() && CLAUDE_KEPT_FILES.contains(&name),
        };
        let second =
            kept && file_type.is_file() && name != LOGIN_FILE && has_second_name(&entry.path())?;
        if second {
            tracing::warn!("a kept state file had a second name; it is removed");
        }
        if !kept || second {
            remove_entry(&entry.path(), file_type)?;
        } else if kind == AgentKind::Codex && file_type.is_dir() {
            sweep_links_by_path(&entry.path())?;
        }
    }
    Ok(())
}

/// Off Unix: in one of Codex's kept folders, removes at any depth every file with a second name
/// and every link or junction (removed as itself, never descended into).
#[cfg(not(unix))]
fn sweep_links_by_path(folder: &Path) -> io::Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            sweep_links_by_path(&entry.path())?;
        } else if !kind.is_file() || has_second_name(&entry.path())? {
            remove_entry(&entry.path(), kind)?;
        }
    }
    Ok(())
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
    prepare_with(data, state, kind, true)
}

/// [`prepare`] for a launch while another session of the agent runs in the matter: the live
/// clean ([`clean`] with `live`), which keeps that session's working files (its tool results,
/// plan note, to-dos) but still removes every instruction surface; the folder is made, Codex's
/// `tmp` made and Claude's `.claude.json` rewritten all the same. The next launch with no other
/// session running there cleans to the strict keep list.
pub(crate) fn prepare_in_use(data: &Path, state: &Path, kind: AgentKind) -> Result<(), Error> {
    prepare_with(data, state, kind, false)
}

/// One lock per state folder, so two launches' cleans of one matter's folder never run at once
/// (each on its own launch-prep thread), nor a launch's link with a login change's removal of
/// it. Held for the make, the clean and the config rewrite, and, apart, for the stale link's
/// removal and the launch's link (`launch_checks/state_prep.rs`) and for each removal by
/// `login_link::remove_links_but`. The map keeps one small entry per state folder a host run has
/// prepared and never shrinks (the named residual).
pub(crate) fn folder_lock(state: &Path) -> std::sync::Arc<std::sync::Mutex<()>> {
    use std::collections::BTreeMap;
    use std::sync::{Arc, LazyLock, Mutex, PoisonError};
    static LOCKS: LazyLock<Mutex<BTreeMap<PathBuf, Arc<Mutex<()>>>>> =
        LazyLock::new(|| Mutex::new(BTreeMap::new()));
    let mut locks = LOCKS.lock().unwrap_or_else(PoisonError::into_inner);
    Arc::clone(locks.entry(state.to_owned()).or_default())
}

/// [`prepare`], with the strict clean when `cleaned`, else the live one, under the folder's
/// lock ([`folder_lock`]).
fn prepare_with(data: &Path, state: &Path, kind: AgentKind, cleaned: bool) -> Result<(), Error> {
    let lock = folder_lock(state);
    let _held = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let who = match kind {
        AgentKind::Claude => "Claude Code",
        AgentKind::Codex => "Codex",
    };
    make(data, state, who)?;
    let first = clean(state, kind, !cleaned);
    let cleaned = first.and_then(|()| match kind {
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
mod live_tests;
#[cfg(test)]
mod tests;
