//! What Clauseo's Claude home holds that Claude Code loads instructions, settings, skills or
//! code from, and that Clauseo does not write itself, removed at every Claude launch
//! ([`clear_home`]) as the last step of the launch's preparation, as Codex's are
//! (`agent/codex/home_clear.rs`). The names come from the config-folder names Claude Code
//! 2.1.293 reads (`data_fence/claude_home.rs`). Claude Code watches `skills`, `commands` and
//! `agents` for changes during a session only when they exist at its start, so each is removed
//! whole, not emptied. The files it reads settings from ([`SETTINGS`]) also go before each
//! sign-in, status check and logout ([`clear_settings`]), with the configuration in its global
//! config (`super::claude_json`).
//!
//! Never touched, so a lawyer loses nothing: the sign-in (`.credentials.json`), Claude Code's
//! own state (`.claude.json` less its configuration, `backups`, `projects`, `plans`,
//! `sessions`, `todos`, caches and logs), and `settings.json`, which the launch writes itself
//! (`super::write_claude_settings`).

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::log_file::Cause;

/// The files in Claude's home that Claude Code 2.1.293 reads settings from, other than the
/// `settings.json` Clauseo writes and its global config, each removed before every Claude Code
/// process Clauseo starts: an account's remote settings and policy limits are fetched afresh at
/// its start.
pub(crate) const SETTINGS: [&str; 12] = [
    // Read in place of `.claude.json` when it exists, its `env` applied the same way.
    ".config.json",
    // The user settings file in place of `settings.json` when its cowork mode is on.
    "cowork_settings.json",
    // Local user settings, read beside `settings.json`.
    "settings.local.json",
    // The cached remote settings of an account they apply to, with their signature files and
    // consent records.
    "remote-settings.json",
    "remote-settings.json.signature.json",
    "remote-settings.json.signature-iat.json",
    "remote-settings-helper-consent",
    "remote-settings-consent.json",
    // The cached policy limits, with their signature files and stamp.
    "policy-limits.json",
    "policy-limits.json.signature.json",
    "policy-limits.json.signature-iat.json",
    "policy-limits.json.stamp.json",
];

/// The names in Claude's home that it loads instructions, skills or code from, each removed at
/// every launch with [`SETTINGS`].
pub(crate) const LOADED: [&str; 10] = [
    // Instructions for every session.
    "CLAUDE.md",
    // The user's skills, slash commands and subagents, watched during a session when present.
    "skills",
    "commands",
    "agents",
    // Rules and output styles, made into instructions.
    "rules",
    "output-styles",
    // Plugins the user installed, with their skills, hooks, agents and servers.
    "plugins",
    // Hooks and workflows.
    "hooks",
    "workflows",
    // Subagents' memory, made into instructions.
    "agent-memory",
];

/// The start of the name an entry is renamed to before it is removed: a name Claude Code loads
/// nothing from. An entry that could not be removed stays under it until a later launch.
pub(crate) const ASIDE: &str = ".clauseo-aside-";

/// The most entries left aside by earlier launches that one clear removes; the rest wait for
/// later ones, so a home filled with them cannot hold a launch up.
const MAX_SWEPT: usize = 256;

/// Moves each of [`SETTINGS`] and [`LOADED`] out of Claude Code's sight before it starts, at
/// every launch, whoever wrote it, and removes the configuration from its global config
/// ([`super::claude_json::strip_config`]). First the home is checked again to be a real folder
/// under `data` ([`still_real`]), so nothing a link put there since the launch made it leads to
/// is touched.
/// Each name is renamed aside in `home` ([`set_aside`]), then removed, a folder with all it
/// holds, a file, or a link (never followed). The removal is best-effort: what can't be removed
/// stays aside, and the next launch tries again. Only a name that can't be renamed, or a global
/// config that can't be written, refuses the launch, as a home that cannot be made does: Claude
/// Code would load it. Nothing there is fine, and so is a name another launch moved first.
/// `tracing` gets counts, never names, contents or the path.
pub(crate) fn clear_home(data: &Path, home: &Path) -> Result<(), Error> {
    still_real(data, home)
        .and_then(|()| clear_names(home, SETTINGS.into_iter().chain(LOADED)))
        .map_err(|error| clear_failed(&error))
}

/// Whether each folder from below `data` down to `home` is still a real folder, as the launch
/// made it (`codex::make_agent_home`): a link or junction put there since is an error, so the
/// clear never works through one. `symlink_metadata` never follows a link, and on Windows a
/// junction is a link to it.
fn still_real(data: &Path, home: &Path) -> std::io::Result<()> {
    let below = home
        .strip_prefix(data)
        .map_err(|_outside| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let mut at = data.to_owned();
    for part in below.components() {
        at.push(part);
        if !std::fs::symlink_metadata(&at)?.is_dir() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory));
        }
    }
    Ok(())
}

/// Before a sign-in, status check or logout's Claude Code process: removes each of
/// [`SETTINGS`] from `home` and the configuration from its global config, as a launch does.
/// First the home and the two folders above it (`<data>/agents/<id>/home`, as
/// `Locations::agent_home` lays it out) are checked to be real folders ([`still_real`]), so
/// nothing a link put there leads to is touched. Nothing there is fine; an error when a folder
/// is a link, or a name can be neither renamed nor its config written.
pub(crate) fn clear_settings(home: &Path) -> std::io::Result<()> {
    let data = home
        .ancestors()
        .nth(3)
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    still_real(data, home)?;
    clear_names(home, SETTINGS)
}

/// Strips the global config in `home`, then sets aside and removes each of `names` there.
fn clear_names(home: &Path, names: impl IntoIterator<Item = &'static str>) -> std::io::Result<()> {
    super::claude_json::strip_config(home)?;
    let mut left = sweep_aside(home);
    let mut moved = 0_usize;
    for name in names {
        let Some(aside) = set_aside(home, name)? else {
            continue;
        };
        moved = moved.saturating_add(1);
        if remove_entry(&aside).is_err() {
            left = left.saturating_add(1);
        }
    }
    if moved > 0 || left > 0 {
        tracing::warn!(
            moved,
            left,
            "cleared what Claude Code would load from its home"
        );
    }
    Ok(())
}

/// Renames `home`'s entry `name` to a fresh [`ASIDE`] name there and gives that name; none when
/// nothing is there (or another launch moved it first).
pub(crate) fn set_aside(home: &Path, name: &str) -> std::io::Result<Option<PathBuf>> {
    let path = home.join(name);
    let meta = match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        other => other?,
    };
    let aside = fresh_aside(home)?;
    let kind = meta.file_type();
    let renamed = if kind.is_dir() && !kind.is_symlink() {
        crate::write::rename_folder(&path, &aside)
    } else {
        crate::write::rename_file(&path, &aside)
    };
    match renamed {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        other => other.map(|()| Some(aside)),
    }
}

/// A name in `home` that is free: [`ASIDE`] and 16 random hex digits.
fn fresh_aside(home: &Path) -> std::io::Result<PathBuf> {
    for _ in 0..8 {
        let mut bytes = [0_u8; 8];
        getrandom::fill(&mut bytes).map_err(std::io::Error::other)?;
        let aside = home.join(format!("{ASIDE}{:016x}", u64::from_ne_bytes(bytes)));
        if std::fs::symlink_metadata(&aside).is_err() {
            return Ok(aside);
        }
    }
    Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists))
}

/// Removes what earlier launches left aside in `home`, best-effort and at most [`MAX_SWEPT`] of
/// them; how many it found still there.
fn sweep_aside(home: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(home) else {
        return 0;
    };
    let mut left = 0_usize;
    let aside = entries.flatten().filter(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(ASIDE))
    });
    for entry in aside.take(MAX_SWEPT) {
        if remove_entry(&entry.path()).is_err() {
            left = left.saturating_add(1);
        }
    }
    left
}

/// Removes `path` whatever it is: a real folder with all it holds, else (a file, a link, a
/// junction) the entry itself. Nothing there is fine.
pub(crate) fn remove_entry(path: &Path) -> std::io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        other => other?,
    };
    let kind = meta.file_type();
    // A link (a junction too, on Windows) is never taken for a folder, whatever it points to.
    let removed = if kind.is_dir() && !kind.is_symlink() {
        crate::write::remove_tree(path)
    } else {
        crate::write::remove_link(path)
    };
    match removed {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// The failure of a Claude launch whose home could not be cleared: Clauseo's plain words for a
/// home it could not prepare, the io error's kind in the log.
fn clear_failed(error: &std::io::Error) -> Error {
    tracing::warn!(error_kind = ?error.kind(), "Claude Code's home could not be cleared");
    crate::agent::codex::agent_home_failed("Claude Code", &Cause(error))
}

#[cfg(test)]
mod tests;
