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
//! followed). Before every launch the config surfaces the agent reads from its folder are
//! taken back ([`clean`]): what the session may have planted there (a prompt-injected agent's
//! hooks, instructions or skills) never reaches the matter's later sessions. The lists, with
//! where each name was confirmed, are [`CLAUDE_REMOVED`] and [`CODEX_REMOVED`]; Clauseo's own
//! files there (`settings.json`, `config.toml`) are written again after the clean. Transcripts
//! (`projects/`, `sessions/`, `todos/` and the rest) are kept, so `session/load` finds them.
//! The jail's write-deny of these names during a session is R17 PR-D's.
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

pub(crate) use file_id::same_file;

/// The folder under `<data>` that holds every matter's state folders and notes.
pub(crate) const STATE_ROOT: &str = "agent-state";

/// Codex's login file in its home and in each state folder.
pub(crate) const LOGIN_FILE: &str = "auth.json";

/// The temporary name a re-link is made at before it is renamed over [`LOGIN_FILE`], so a
/// Codex already running in the matter never finds the name missing.
const RELINK_NAME: &str = "auth.json.clauseo-link";

/// What of Claude Code's config folder is removed before every launch: each name read from
/// `CLAUDE_CONFIG_DIR` that can change what a session loads or runs. `settings.json` is not
/// here: it is written whole, Clauseo's keys only ([`super::home::write_state_settings`]).
/// Sources (Claude Code 0.3.293 under claude-agent-acp, `research/42-reusing-the-claude-code-
/// login.md` §2, bundle offsets): `settings.local.json` (Claude Code's local settings file,
/// docs `code.claude.com/docs/en/settings`; not confirmed read from the config folder, removed
/// as a settings file all the same); `CLAUDE.md` (`Ve(we(),…)` @186994727); `rules`
/// (`MLe()` @186994752); `skills` (@192585549; claude.ai synced skills in `skills/synced`);
/// `plugins` (@188170636, @188224681: installed plugins bring hooks, agents, commands, MCP);
/// `agents` (docs `~/.claude/agents`; the exact path function not confirmed); `commands` and
/// `output-styles` (docs `~/.claude/commands`, `~/.claude/output-styles`; not confirmed in the
/// bundle); `hooks` (no loader reads it, hooks live in `settings.json`; removed since a hook
/// command names scripts there by convention). Clauseo writes no file into any of them: its
/// skills are its plugin under `<data>/profile/claude-plugin` (`clauseo_profile`), passed by
/// path. `.claude.json` is kept (onboarding, trust list, the account's profile): its
/// `mcpServers` are kept out by `strictMcpConfig` (`agent/loads.rs`).
pub(crate) const CLAUDE_REMOVED: [&str; 9] = [
    "settings.local.json",
    "CLAUDE.md",
    "rules",
    "skills",
    "plugins",
    "agents",
    "commands",
    "output-styles",
    "hooks",
];

/// What of Codex's home is removed before every launch. `config.toml` is not here: it is
/// written whole ([`super::codex::write_home_config`]). Sources (`research/43-reusing-the-codex-
/// login.md` §2 at `rust-v0.162.1`, and a local codex-rs checkout for `hooks.json`):
/// `AGENTS.md` and `AGENTS.override.md` (`codex-home/src/instructions/mod.rs:12-13, 41-85`,
/// `core/src/agents_md.rs:37-39`), `rules` (`core/src/exec_policy.rs:49, 662-682`), `skills`
/// (`ext/skills/src/host_roots.rs:80-107`), `hooks.json` (`hooks/src/engine/discovery.rs:
/// 129-150, 259`: Clauseo's guard turns hooks on), `plugins` (`core-plugins/src/manager.rs:
/// 2814-2824`; off by `features.plugins=false`, removed all the same), `prompts` (custom
/// prompts; not found in the checkout, removed as the brief names it). With `HOME` the state
/// folder on macOS and Linux: `.agents` (Codex adds `~/.agents/skills`, research 43:61) and
/// the shell and git start-up files a `bash -lc` or `git` run by Codex's shell reads from
/// `HOME`. Clauseo writes no file into any of them: the profile's Codex skills are off
/// (`clauseo_profile::CODEX_SKILLS`), and would go in the login home.
pub(crate) const CODEX_REMOVED: [&str; 16] = [
    "AGENTS.md",
    "AGENTS.override.md",
    "rules",
    "skills",
    "hooks.json",
    "plugins",
    "prompts",
    ".agents",
    ".profile",
    ".bash_profile",
    ".bash_login",
    ".bashrc",
    ".zshenv",
    ".zprofile",
    ".zshrc",
    ".gitconfig",
];

/// The names [`clean`] removes for an agent of `kind`.
pub(crate) fn removed(kind: AgentKind) -> &'static [&'static str] {
    match kind {
        AgentKind::Claude => &CLAUDE_REMOVED,
        AgentKind::Codex => &CODEX_REMOVED,
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

/// Removes each of `names` from `state`, through no link: a real folder is removed with all it
/// holds (`remove_dir_all`, which removes a link inside and never follows it), anything else
/// (a file, a link, a junction) is removed as itself. A missing name is fine; the first other
/// failure fails the clean, and so the launch. File system calls: off the actor.
pub(crate) fn clean(state: &Path, names: &[&str]) -> io::Result<()> {
    for name in names {
        let path = state.join(name);
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            Ok(meta) if meta.is_dir() => crate::write::remove_tree(&path)?,
            Ok(_) => match crate::write::remove_link(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                other => other?,
            },
        }
    }
    Ok(())
}

/// Makes and cleans `kind`'s state folder `state` under `data`: [`make`], then [`clean`]. The
/// caller writes Clauseo's own file there next.
pub(crate) fn prepare(data: &Path, state: &Path, kind: AgentKind) -> Result<(), Error> {
    let who = match kind {
        AgentKind::Claude => "Claude Code",
        AgentKind::Codex => "Codex",
    };
    make(data, state, who)?;
    clean(state, removed(kind)).map_err(|error| {
        tracing::warn!(who, error = %Cause(&error), "an agent's state folder was not cleaned");
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            format!("Clauseo couldn't prepare {who}'s folder."),
        )
    })
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
/// whose `auth.json` `HostHandle::remove_login_links` removes. `agent-state` and each matter's
/// folder must be real folders: a link is skipped, never followed. Only Codex links its login;
/// any other agent has none. File system calls: off the actor.
pub(crate) fn linked_folders(data: &Path, agent: &str) -> Vec<PathBuf> {
    if agent != AgentKind::Codex.id() {
        return Vec::new();
    }
    let root = state_root(data);
    if !crate::write::is_real_dir(&root) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path().join(agent))
        .filter(|folder| crate::write::is_real_dir(folder))
        .collect();
    folders.sort();
    folders
}

#[cfg(test)]
mod tests;
