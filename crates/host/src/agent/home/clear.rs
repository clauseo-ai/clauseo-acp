//! What Clauseo's Claude home holds that Claude Code loads instructions, settings, skills or
//! code from, and that Clauseo does not write itself, removed at every Claude launch
//! ([`clear_home`]) as the last step of the launch's preparation, as Codex's are
//! (`agent/codex/home_clear.rs`). The names come from the config-folder names Claude Code
//! 2.1.293 reads (`data_fence/claude_home.rs`). Claude Code watches `skills`, `commands` and
//! `agents` for changes during a session only when they exist at its start, so each is removed
//! whole, not emptied.
//!
//! Never touched, so a lawyer loses nothing: the sign-in (`.credentials.json`), Claude Code's
//! own state (`.claude.json`, `backups`, `projects`, `plans`, `sessions`, `todos`, caches and
//! logs), and `settings.json`, which the launch writes itself (`super::write_claude_settings`).

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::log_file::Cause;

/// The names in Claude's home that it loads from, each removed at every launch.
pub(crate) const LOADED: [&str; 11] = [
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
    // Local user settings, read beside `settings.json`.
    "settings.local.json",
];

/// The start of the name an entry is renamed to before it is removed: a name Claude Code loads
/// nothing from. An entry that could not be removed stays under it until a later launch.
pub(crate) const ASIDE: &str = ".clauseo-aside-";

/// Moves each of [`LOADED`] out of Claude Code's sight before it starts, at every launch,
/// whoever wrote it: renamed aside in `home` ([`set_aside`]), then removed, a folder with all
/// it holds, a file, or a link (never followed). The removal is best-effort: what can't be
/// removed stays aside, and the next launch tries again. Only a name that can't be renamed
/// refuses the launch, as a home that cannot be made does: Claude Code would load it. Nothing
/// there is fine, and so is a name another launch moved first. `tracing` gets counts, never
/// names, contents or the path.
pub(crate) fn clear_home(home: &Path) -> Result<(), Error> {
    let mut left = sweep_aside(home);
    let mut moved = 0_usize;
    for name in LOADED {
        let Some(aside) = set_aside(home, name).map_err(|error| clear_failed(&error))? else {
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

/// Removes what earlier launches left aside in `home`, best-effort; how many are still there.
fn sweep_aside(home: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(home) else {
        return 0;
    };
    let mut left = 0_usize;
    for entry in entries.flatten() {
        let aside = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(ASIDE));
        if aside && remove_entry(&entry.path()).is_err() {
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
