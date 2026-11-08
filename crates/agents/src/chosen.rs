//! Resume after a relaunch: the sign-in flag and what an install would still fetch.
//!
//! - The flag, `<data_dir>/agents/<id>/.chosen`, says the lawyer pressed Sign in for the agent
//!   and has not finished signing in. The desktop writes it on a Sign in press
//!   ([`crate::Agents::mark_chosen`]) and clears it on a successful sign-in, a cancel and a
//!   sign-out ([`crate::Agents::clear_chosen`]); the worker clears it at start for an agent IT
//!   manages, or one the snapshot no longer has. Its content is fixed, with no personal data.
//!   Every write and removal is best-effort: an error is logged, never the lawyer's failure.
//! - [`on_disk`] reads what of an install's files is in `agents/downloads/` already, each file
//!   hashed against its digest (never deleted here: this runs outside `agents/.lock`). It gives
//!   the `Missing` card's bytes still to fetch, and whether a flagged agent's install can run
//!   at start as a set-up only, with nothing to fetch.

use std::path::{Path, PathBuf};

use clauseo_protocol::AgentId;

use crate::config::AgentsConfig;
use crate::download::hash_file;
use crate::integrity::Expected;
use crate::write;

/// The flag's file name, beside the agent's `install/`.
const FLAG: &str = ".chosen";

/// The flag's content: fixed, so it never holds anything about the lawyer.
const CONTENT: &[u8] = b"sign-in pressed\n";

/// `<data_dir>/agents/<id>/.chosen`.
fn flag_path(data_dir: &Path, agent: &AgentId) -> PathBuf {
    data_dir.join("agents").join(agent.as_str()).join(FLAG)
}

/// Writes `agent`'s flag. A relative data folder is never touched.
pub(crate) fn mark(data_dir: &Path, agent: &AgentId) {
    if !data_dir.is_absolute() {
        return;
    }
    let path = flag_path(data_dir, agent);
    let written = path
        .parent()
        .map_or(Ok(()), |dir| {
            write::create_private_dirs_under(data_dir, dir)
        })
        .and_then(|()| write::write_atomic(&path, CONTENT));
    if let Err(err) = written {
        tracing::warn!(%agent, kind = ?err.kind(), "the sign-in flag was not written");
    }
}

/// Removes `agent`'s flag; none there is fine.
pub(crate) fn clear(data_dir: &Path, agent: &AgentId) {
    if !data_dir.is_absolute() {
        return;
    }
    if let Err(err) = write::remove_file(&flag_path(data_dir, agent)) {
        tracing::warn!(%agent, kind = ?err.kind(), "the sign-in flag was not removed");
    }
}

/// Whether `agent`'s flag is there, as a regular file (a link does not count).
pub(crate) fn is_marked(data_dir: &Path, agent: &AgentId) -> bool {
    std::fs::symlink_metadata(flag_path(data_dir, agent)).is_ok_and(|meta| meta.is_file())
}

/// Removes the flag of every agent folder under `agents/` the snapshot no longer has.
pub(crate) fn clear_unknown(config: &AgentsConfig) {
    let Ok(entries) = std::fs::read_dir(config.data_dir.join("agents")) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if config.snapshot.agents().any(|e| e.id.as_str() == name) {
            continue;
        }
        let flag = entry.path().join(FLAG);
        if std::fs::symlink_metadata(&flag).is_ok_and(|meta| meta.is_file())
            && let Err(err) = write::remove_file(&flag)
        {
            tracing::warn!(kind = ?err.kind(), "a removed agent's sign-in flag was not removed");
        }
    }
}

/// What of an install's files is on disk already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OnDisk {
    /// Every tarball, and Node's archive when Node would be downloaded, is there and checked:
    /// the install fetches nothing.
    pub(crate) complete: bool,
    /// Bytes still to fetch: 0 when `complete`; `None` when the snapshot has no size for the
    /// agent on this platform.
    pub(crate) to_fetch: Option<u64>,
}

/// Reads which of `agent`'s files are in `agents/downloads/` with the right digest, Node's
/// archive too when `node_needed`.
pub(crate) fn on_disk(config: &AgentsConfig, agent: &AgentId, node_needed: bool) -> OnDisk {
    let Some(entry) = config.snapshot.agent(agent) else {
        return OnDisk {
            complete: false,
            to_fetch: None,
        };
    };
    let downloads = crate::install::downloads_dir(&config.data_dir);
    let mut tarballs_complete = true;
    let mut present = 0u64;
    for package in config.snapshot.packages(agent, config.platform) {
        let dest = downloads.join(format!("{}.tgz", package.integrity.hex()));
        match checked_size(&dest, &package.integrity) {
            Some(size) => present = present.saturating_add(size),
            None => tarballs_complete = false,
        }
    }
    let (node_complete, node_left) = if node_needed {
        match config.snapshot.node_archive(config.platform) {
            Some(archive) => {
                let dest = downloads.join(archive.name());
                match checked_size(&dest, archive.expected()) {
                    Some(_) => (true, 0),
                    None => (false, archive.size()),
                }
            }
            // No Node for this platform: the install cannot run, so nothing is complete.
            None => (false, 0),
        }
    } else {
        (true, 0)
    };
    let complete = tarballs_complete && node_complete;
    if complete {
        return OnDisk {
            complete,
            to_fetch: Some(0),
        };
    }
    let agent_left = if tarballs_complete {
        Some(0)
    } else {
        // The row is the whole platform's total, with no size per package: what is here comes
        // off it, and a tarball still missing is never shown as nothing left.
        entry
            .sizes_for(config.platform)
            .map(|row| row.download_bytes.saturating_sub(present).max(1))
    };
    let to_fetch = agent_left.map(|left| left.saturating_add(node_left));
    OnDisk { complete, to_fetch }
}

/// The size of the regular file at `path` when its digest is `expected`. Nothing is deleted.
fn checked_size(path: &Path, expected: &Expected) -> Option<u64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mut hasher = expected.hasher();
    let size = hash_file(path, &mut hasher).ok()?;
    hasher.matches(expected).then_some(size)
}
