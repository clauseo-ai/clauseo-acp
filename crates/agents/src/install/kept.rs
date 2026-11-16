//! A finished set-up kept for its rename alone. Once `npm ci` and the link check have passed
//! in `install/.tmp-<hash>-<n>`, a marker is written there ([`mark`]): the agent, the adapter's
//! version, the install's `<hash>` (agent, derived lock and platform) and a manifest of every
//! file in the tree (each path and size, hashed). When the rename into place then fails, even
//! after its in-place retries (a folder walker holding the fresh folder), the tree is kept.
//! The next try, the job's own retry or the lawyer's Retry, re-checks it ([`find`]): the
//! marker names this agent, version and `<hash>`, the tree has no link, the adapter's script is
//! there, and the manifest read now matches the marker's. Then only the rename runs
//! ([`finish_kept`]): no npm cache emptying, no `npm ci`, no download, and the state stays
//! `Installing`. A tree that fails any check is removed and the try runs in full.
//!
//! The start's stale clean-up ([`crate::receipt::scan`]) keeps a tree whose marker names the
//! current `<hash>` ([`marked_for`]); any other `.tmp-*` goes, so another version's tree does.

use std::io;
use std::path::{Path, PathBuf};

use clauseo_process::Timer;
use clauseo_protocol::AgentId;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::npm::{first_link, missing_agent, rename_held};
use super::{Job, NodeChoice, TryError};
use crate::integrity::to_hex;
use crate::receipt::{Receipt, install_folder, install_root, script_path, write_receipt};
use crate::snapshot::AgentEntry;
use crate::state::{InstallState, Installed};
use crate::write;

/// The marker's file name in a finished temp tree.
pub(crate) const MARKER: &str = ".clauseo-complete";

/// What the marker holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Marker {
    agent: String,
    version: String,
    lock_hash: String,
    /// SHA-256 of every file's `<path>\t<size>\n` below the tree, sorted, the marker left out.
    manifest: String,
}

/// Marks `tmp` finished, for `agent` at adapter `version` and `<hash>` `hash`: written only
/// after `npm ci` and the link check passed.
pub(super) fn mark(tmp: &Path, agent: &AgentId, version: &str, hash: &str) -> io::Result<()> {
    let manifest = manifest(tmp).ok_or_else(|| io::Error::other("the tree could not be read"))?;
    let marker = Marker {
        agent: agent.as_str().to_owned(),
        version: version.to_owned(),
        lock_hash: hash.to_owned(),
        manifest,
    };
    let bytes = serde_json::to_vec(&marker).map_err(io::Error::other)?;
    write::write_atomic(&tmp.join(MARKER), &bytes)
}

/// The marker in `tmp`, when it reads.
fn read_marker(tmp: &Path) -> Option<Marker> {
    let bytes = std::fs::read(tmp.join(MARKER)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Whether `tmp`, a `.tmp-*` folder of `agent`, holds a marker for `<hash>` `hash`: kept by
/// the start's clean-up for the next try's full check.
pub(crate) fn marked_for(tmp: &Path, agent: &AgentId, hash: &str) -> bool {
    read_marker(tmp)
        .is_some_and(|marker| marker.agent == agent.as_str() && marker.lock_hash == hash)
}

/// The kept tree of `agent` at `version` and `hash` under `root`, re-checked in full; every
/// `.tmp-<hash>-*` folder that fails the check is removed. `None` when there is none.
pub(super) fn find(
    root: &Path,
    agent: &AgentId,
    version: &str,
    hash: &str,
    entry: &AgentEntry,
) -> Option<PathBuf> {
    let prefix = format!(".tmp-{hash}-");
    let mut found = None;
    for item in std::fs::read_dir(root).ok()?.flatten() {
        let path = item.path();
        if !item.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let real_dir = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir());
        if found.is_none() && real_dir && sound(&path, agent, version, hash, entry) {
            found = Some(path);
        } else {
            tracing::info!(%agent, "agent install: a kept set-up failed its check; removed");
            drop(write::remove_any(&path));
        }
    }
    found
}

/// The full check of a kept tree: the marker for this agent, version and `<hash>`, no link,
/// the adapter's script, and the manifest read now equal to the marker's.
fn sound(tmp: &Path, agent: &AgentId, version: &str, hash: &str, entry: &AgentEntry) -> bool {
    let Some(marker) = read_marker(tmp) else {
        return false;
    };
    marker.agent == agent.as_str()
        && marker.version == version
        && marker.lock_hash == hash
        && first_link(tmp).is_none()
        && script_path(tmp, entry).is_file()
        && manifest(tmp).is_some_and(|now| now == marker.manifest)
}

/// The manifest of `dir`: SHA-256 hex over every file's `<path>\t<size>\n`, sorted, with
/// `/` separators and the marker left out. `None` when a folder will not read or a link stands
/// anywhere.
fn manifest(dir: &Path) -> Option<String> {
    let mut lines = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        for item in std::fs::read_dir(&folder).ok()? {
            let path = item.ok()?.path();
            let meta = std::fs::symlink_metadata(&path).ok()?;
            if meta.file_type().is_symlink() {
                return None;
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let below = path.strip_prefix(dir).ok()?;
            if below == Path::new(MARKER) {
                continue;
            }
            let parts: Vec<_> = below
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect();
            lines.push(format!("{}\t{}\n", parts.join("/"), meta.len()));
        }
    }
    lines.sort();
    let mut hasher = Sha256::new();
    for line in &lines {
        hasher.update(line.as_bytes());
    }
    Some(to_hex(&hasher.finalize()))
}

/// The rename-only try: when a kept tree passes its check, `Installing`, the rename (with its
/// in-place retries) and the receipt; `None` when there is no sound kept tree, or when the
/// Node has to be fetched (a full try unpacks it). A failed rename keeps the tree again.
pub(super) async fn finish_kept<T: Timer + Send + Sync>(
    job: &Job<T>,
    node: &NodeChoice,
) -> Result<Option<Installed>, TryError> {
    let config = &job.config;
    let agent = &job.ticket.agent;
    let snapshot = &config.snapshot;
    let (program, version) = match node {
        NodeChoice::User(found) => (found.program.clone(), found.version.clone()),
        NodeChoice::Runtime { fetch: true, .. } => return Ok(None),
        NodeChoice::Runtime { dir, .. } => (
            crate::node::runtime_program(dir, config.platform.os),
            format!("v{}", snapshot.node().version()),
        ),
    };
    let entry = snapshot.agent(agent).ok_or_else(|| missing_agent(config))?;
    let pins = snapshot.pins(agent).ok_or_else(|| missing_agent(config))?;
    let hash = snapshot
        .install_hash(agent, config.platform)
        .ok_or_else(|| missing_agent(config))?;
    let root = install_root(&config.data_dir, agent);
    if !write::real_dirs_under(&config.data_dir, &root).unwrap_or(false) {
        return Ok(None);
    }
    let Some(tmp) = find(&root, agent, pins.version(), &hash, entry) else {
        return Ok(None);
    };
    tracing::info!(%agent, "agent install: a finished set-up was kept; only its rename runs");
    // The receipt names the downloaded Node: it must be there, and another try fetching it
    // (`.lock-node` held) is waited for, as a full try's npm waits.
    if matches!(node, NodeChoice::Runtime { .. }) {
        job.use_node().await?;
    }
    if !job.emit(InstallState::Installing, false) {
        return Err(TryError::cancelled());
    }
    let placed = Placed {
        hash: &hash,
        version: pins.version(),
        entry,
        program: &program,
        node_version: &version,
    };
    place(job, &tmp, &placed).await.map(Some)
}

/// What [`place`] writes into the receipt and the [`Installed`].
pub(super) struct Placed<'a> {
    pub(super) hash: &'a str,
    /// The adapter's version.
    pub(super) version: &'a str,
    pub(super) entry: &'a AgentEntry,
    pub(super) program: &'a Path,
    pub(super) node_version: &'a str,
}

/// Renames the finished, marked `tmp` to its `<hash>` folder (retried in place), drops the
/// marker there and writes the receipt, last.
pub(super) async fn place<T: Timer + Send + Sync>(
    job: &Job<T>,
    tmp: &Path,
    placed: &Placed<'_>,
) -> Result<Installed, TryError> {
    let config = &job.config;
    let agent = &job.ticket.agent;
    let folder = install_folder(&config.data_dir, agent, placed.hash);
    write::remove_dir_all(&folder)
        .map_err(|err| TryError::io("clearing the install folder", &err, config))?;
    rename_held(job, tmp, &folder).await?;
    drop(write::remove_file(&folder.join(MARKER)));
    let receipt = Receipt::new(
        agent,
        placed.version,
        placed.hash,
        placed.program,
        placed.node_version,
        config.platform,
        config.clock.now(),
    );
    write_receipt(&folder, &receipt)
        .map_err(|err| TryError::io("writing the receipt", &err, config))?;
    Ok(Installed {
        node: placed.program.to_owned(),
        script: script_path(&folder, placed.entry),
        folder,
        version: placed.version.to_owned(),
    })
}
