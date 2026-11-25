//! What a Codex agent's home holds that Codex loads instructions, settings or code from at its
//! start, removed at every launch ([`clear_home`]), as its `rules` are (`rules.rs`). The names
//! are read from codex `rust-v0.162.1` (`codex-rs/`, cited per name in [`LOADED`]).
//!
//! Never touched, so a lawyer loses nothing: the sign-in (`auth.json`, `secrets`,
//! `.credentials.json`), the conversations (`sessions`, `archived_sessions`,
//! `session_index.jsonl`, `history.jsonl`), Codex's databases (`*.sqlite` and their `-wal`,
//! `-shm` and `-journal` files) and logs (`log`), `installation_id` and Codex's lock folders;
//! nor `config.toml`, which the launch writes itself (`super::write_home_config`). Its scope is
//! Codex's home; what Codex loads from elsewhere (`$HOME/.agents/skills`, `/etc/codex`, macOS
//! managed preferences) is handled elsewhere. Codex's bundled skills, plugins and apps caches
//! are switched off by `clauseo_limits::CODEX_OVERRIDES` instead.
//!
//! The cloud config cache only spares a business, education or enterprise sign-in a fetch:
//! without it Codex fetches at its start, waits at most 20 s, and fails the session start when
//! the fetch fails, so a firm's cloud settings are never skipped (`cloud-config/src/service.rs:
//! 37-38, 142-165, 199-223`, `config/src/loader/mod.rs:211-214`).

use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::log_file::Cause;

use super::home_failed;

/// The names in Codex's home that it loads from, each removed at every launch.
pub(crate) const LOADED: [&str; 13] = [
    // Instructions for every session, trusted folder or not
    // (`codex-home/src/instructions/mod.rs:41-44`).
    "AGENTS.override.md",
    "AGENTS.md",
    // The user's skills (`ext/skills/src/host_roots.rs:95-101`).
    "skills",
    // Subagent roles, each with its instructions (`agent-roles/src/loader.rs:75-81`).
    "agents",
    // Variables for Codex's own process (`arg0/src/lib.rs:299-309`).
    ".env",
    // Where Codex runs commands (`exec-server/src/environment_toml.rs:234-245`).
    "environments.toml",
    // The models' list with their base instructions, cached (`models-manager/src/manager.rs:34`,
    // `models-manager/src/cache.rs:203-220`).
    "models_cache.json",
    // A cloud config bundle, hooks among it (`cloud-config/src/cache.rs:28-33`).
    "cloud-config-bundle-cache.json",
    // Hooks (`hooks/src/engine/discovery.rs:148, 339-343`).
    "hooks.json",
    // Memories, made into instructions (`memories/read/src/lib.rs:13-14`,
    // `ext/memories/src/extension.rs:71`, `memories/write/src/control.rs:3-9`).
    "memories",
    "memories_v2",
    "memories_extensions",
    // Windows: the sandbox helper Codex copies there and runs again when its size matches
    // (`windows-sandbox-rs/src/setup.rs:192`, `helper_materialization.rs:85-123`).
    ".sandbox-bin",
];

/// The start of the name an entry of [`LOADED`] is renamed to before it is removed: a name Codex
/// loads nothing from. An entry that could not be removed stays under it until a later launch.
const ASIDE: &str = ".clauseo-aside-";

/// Moves each of [`LOADED`] out of Codex's sight before Codex starts, at every launch, whoever
/// wrote it (but Clauseo's own `AGENTS.md`, [`is_clauseos_own`]):
/// renamed aside in `home` ([`ASIDE`]), then removed, a folder with all it holds, a
/// file, or a link (never followed). The removal is best-effort: what can't be removed (a
/// read-only folder's files, a locked file in a folder) stays aside, and the next launch tries
/// again. Only a name that can't be renamed refuses the launch, as a home that cannot be made
/// does ([`home_failed`]): Codex would load it. Nothing there is fine, and so is a name another
/// launch moved first. `tracing` gets counts, never names, contents or the path.
pub(crate) fn clear_home(home: &Path) -> Result<(), Error> {
    let mut left = sweep_aside(home);
    let mut moved = 0_usize;
    for name in LOADED {
        if is_clauseos_own(home, name) {
            continue;
        }
        let Some(aside) = set_aside(home, name).map_err(|error| clear_failed(&error))? else {
            continue;
        };
        moved = moved.saturating_add(1);
        if remove_entry(&aside).is_err() {
            left = left.saturating_add(1);
        }
    }
    if moved > 0 || left > 0 {
        tracing::warn!(moved, left, "cleared what Codex would load from its home");
    }
    Ok(())
}

/// Whether `home`'s entry `name` is the `AGENTS.md` the launch writes itself, byte for byte
/// (`super::write_home_agents`): it stays, so each launch does not move and write it again.
fn is_clauseos_own(home: &Path, name: &str) -> bool {
    name == super::AGENTS_MD
        && super::holds(
            &home.join(name),
            clauseo_profile::instructions::codex_agents_md().as_bytes(),
        )
}

/// Renames `home`'s entry `name` to a fresh [`ASIDE`] name there and gives that name; none when
/// nothing is there (or another launch moved it first). Also how a `cap_sid` that is not
/// Codex's own goes (`super::cap_sid`).
pub(super) fn set_aside(home: &Path, name: &str) -> std::io::Result<Option<PathBuf>> {
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
pub(super) fn remove_entry(path: &Path) -> std::io::Result<()> {
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

/// The failure of a Codex launch whose home could not be cleared: Clauseo's plain words for a
/// home it could not prepare, the io error's kind in the log.
fn clear_failed(error: &std::io::Error) -> Error {
    tracing::warn!(error_kind = ?error.kind(), "Codex's home could not be cleared");
    home_failed(&Cause(error))
}

#[cfg(test)]
mod tests;
