//! The clear of what earlier builds left of Codex's memories in Clauseo's Codex home, at the
//! start-up sweep (`actor/turn/launch_checks/sweep.rs`), before any launch of this host start.
//!
//! Every matter's Codex sessions share the one home, and Codex's memories there are home-wide:
//! what a session in one matter left could reach another's. Each Codex session now runs with
//! memories off (`clauseo_limits::CODEX_OVERRIDES`), so at codex `rust-v0.162.1` nothing reads
//! these stores: the extraction is gated on the feature (`codex-rs/memories/write/src/start.rs:
//! 33-38`), the prompt's memory text too (`codex-rs/ext/memories/src/extension.rs:48`), and the
//! database's readers are app-server requests codex-acp never sends. They hold only
//! agent-derived data the lawyer does not rely on, so they go: a store left there is still every
//! matter's, and one ungated path still writes it (a reply citing a memory bumps its use count,
//! `codex-rs/core/src/stream_events_utils.rs:137-150`).
//!
//! Once per data folder: Codex makes the databases again, empty, at its next start
//! (`codex-rs/state/src/runtime.rs:173-186` opens and migrates them whatever the feature says),
//! and with memories off they stay empty, so a marker beside the home ([`MARKER`]) keeps the
//! clear from running again. A clear that left anything (a Windows file another program holds
//! open) writes no marker, so it runs again at the next start. It logs counts, never a path.

use std::path::{Path, PathBuf};

use clauseo_protocol::AgentKind;

/// Codex's memory databases (`codex-rs/state/src/sqlite.rs:36, 107-112`).
const DATABASES: [&str; 2] = ["memories_1.sqlite", "memories_v2_1.sqlite"];

/// A SQLite database's own file and its companions.
const SQLITE_SUFFIXES: [&str; 4] = ["", "-wal", "-shm", "-journal"];

/// Codex's memory folders, whose `memory_summary.md` the memories extension puts in the prompt
/// (`codex-rs/protocol/src/memory_version.rs:17-22`, `codex-rs/ext/memories/src/prompts.rs:
/// 39-41`).
const FOLDERS: [&str; 2] = ["memories", "memories_v2"];

/// The marker of a clear done, in `<data>/agents/codex-acp`, beside the home and outside it.
pub(crate) const MARKER: &str = "memories-cleared";

/// Clauseo's Codex home under `data`, as `clauseo_settings::Locations::agent_home` names it.
pub(crate) fn codex_home(data: &Path) -> PathBuf {
    data.join("agents").join(AgentKind::Codex.id()).join("home")
}

/// What [`clear`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Cleared {
    /// Entries removed.
    pub(crate) removed: usize,
    /// Entries that are there still.
    pub(crate) failed: usize,
}

/// Removes Codex's memory databases (each with its companions) and folders from `home`; a link
/// of one of those names is removed itself, never followed. Logs the counts, no path, when
/// anything was there. File system calls: off the actor.
pub(crate) fn clear(home: &Path) -> Cleared {
    let mut cleared = Cleared::default();
    let files = DATABASES.iter().flat_map(|db| {
        SQLITE_SUFFIXES
            .iter()
            .map(move |suffix| format!("{db}{suffix}"))
    });
    for name in files.chain(FOLDERS.iter().map(|name| (*name).to_owned())) {
        let path = home.join(name);
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        let removed = if crate::write::is_real_dir(&path) {
            crate::write::remove_tree(&path)
        } else {
            crate::write::remove_link(&path)
        };
        match removed {
            Ok(()) => cleared.removed += 1,
            Err(_error) => cleared.failed += 1,
        }
    }
    if cleared != Cleared::default() {
        tracing::info!(
            removed = cleared.removed,
            failed = cleared.failed,
            "Codex's memories from earlier runs cleared"
        );
    }
    cleared
}

/// [`clear`] of `data`'s Codex home, unless [`MARKER`] says it was done; the marker is written
/// when nothing is left. Returns what the clear did, `None` when it did not run. File system
/// calls: off the actor.
pub(crate) fn clear_once(data: &Path) -> Option<Cleared> {
    let home = codex_home(data);
    let marker = home.parent()?.join(MARKER);
    if std::fs::symlink_metadata(&marker).is_ok() {
        return None;
    }
    let cleared = clear(&home);
    let parent_there = home.parent().is_some_and(crate::write::is_real_dir);
    if cleared.failed == 0
        && parent_there
        && let Err(_error) = crate::write::replace_private_file(&marker, b"")
    {
        tracing::warn!("the Codex memories clear runs again at the next start");
    }
    Some(cleared)
}

#[cfg(test)]
mod tests;
