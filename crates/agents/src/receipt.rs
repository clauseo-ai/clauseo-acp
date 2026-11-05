//! The install folder's layout and its receipt.
//!
//! An install lives in `<data_dir>/agents/<id>/install/<hash>/` and is complete only once its
//! `clauseo-receipt.json` is there: written atomically as the install's last step, and read
//! back against the agent, the `<hash>` and the platform. At start everything else under
//! `install/` goes: folders with no valid receipt, `.tmp-*` folders and other `<hash>`
//! folders (old pins), since nothing runs from them then.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clauseo_protocol::AgentId;
use serde::{Deserialize, Serialize};

use crate::platform::Platform;
use crate::snapshot::AgentEntry;
use crate::write;

/// The receipt's file name in an install folder.
pub(crate) const RECEIPT: &str = "clauseo-receipt.json";
/// The receipt's schema.
pub(crate) const SCHEMA: u32 = 1;

/// `<data_dir>/agents/<id>/install`: every install folder of the agent.
pub fn install_root(data_dir: &Path, agent: &AgentId) -> PathBuf {
    data_dir.join("agents").join(agent.as_str()).join("install")
}

/// `<data_dir>/agents/<id>/install/<hash>`: one install folder, `hash` from
/// [`crate::Snapshot::install_hash`].
pub fn install_folder(data_dir: &Path, agent: &AgentId, hash: &str) -> PathBuf {
    install_root(data_dir, agent).join(hash)
}

/// The adapter's script in an install folder: `<folder>/node_modules/<root>/<bin>`, joined
/// one part at a time so it is spelled with the OS's separator.
pub fn script_path(folder: &Path, entry: &AgentEntry) -> PathBuf {
    std::iter::once("node_modules")
        .chain(entry.root.split('/'))
        .chain(entry.bin.split('/'))
        .fold(folder.to_path_buf(), |path, part| path.join(part))
}

/// What `clauseo-receipt.json` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Receipt {
    /// [`SCHEMA`].
    pub(crate) schema: u32,
    /// The agent's id.
    pub(crate) agent: String,
    /// The adapter's version.
    pub(crate) version: String,
    /// The install's `<hash>` (agent, derived lock and platform), its folder's name.
    pub(crate) lock_hash: String,
    /// The Node the adapter runs with.
    pub(crate) node: PathBuf,
    /// That Node's `--version`.
    pub(crate) node_version: String,
    /// The platform, as [`Platform`]'s `Display` writes it.
    pub(crate) platform: String,
    /// When the install finished, in Unix seconds.
    pub(crate) installed_at: u64,
}

impl Receipt {
    /// A receipt for a finished install.
    pub(crate) fn new(
        agent: &AgentId,
        version: &str,
        lock_hash: &str,
        node: &Path,
        node_version: &str,
        platform: Platform,
        installed_at: SystemTime,
    ) -> Receipt {
        Receipt {
            schema: SCHEMA,
            agent: agent.as_str().to_owned(),
            version: version.to_owned(),
            lock_hash: lock_hash.to_owned(),
            node: node.to_owned(),
            node_version: node_version.to_owned(),
            platform: platform.to_string(),
            installed_at: installed_at
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        }
    }

    /// Whether it is schema 1 and names this agent, `<hash>` and platform.
    pub(crate) fn matches(&self, agent: &AgentId, lock_hash: &str, platform: Platform) -> bool {
        self.schema == SCHEMA
            && self.agent == agent.as_str()
            && self.lock_hash == lock_hash
            && self.platform == platform.to_string()
    }
}

/// Writes `receipt` into `folder` atomically: the install's last step.
pub(crate) fn write_receipt(folder: &Path, receipt: &Receipt) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(receipt).map_err(io::Error::other)?;
    write::write_atomic(&folder.join(RECEIPT), &bytes)
}

/// `folder`'s receipt when it reads and matches; `None` otherwise (missing, malformed, or for
/// another agent, `<hash>` or platform).
pub(crate) fn read_receipt(
    folder: &Path,
    agent: &AgentId,
    lock_hash: &str,
    platform: Platform,
) -> Option<Receipt> {
    let bytes = std::fs::read(folder.join(RECEIPT)).ok()?;
    let receipt: Receipt = serde_json::from_slice(&bytes).ok()?;
    receipt
        .matches(agent, lock_hash, platform)
        .then_some(receipt)
}

/// What [`scan`] found under an agent's `install/`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Scan {
    /// The current `<hash>` folder, when its receipt is valid.
    pub(crate) keep: Option<(PathBuf, Receipt)>,
    /// Everything else, sorted: to remove.
    pub(crate) remove: Vec<PathBuf>,
}

/// Sorts `<data_dir>/agents/<id>/install/`: the `hash` folder with a valid receipt is kept;
/// every other entry (that folder without a valid receipt, `.tmp-*`, other hashes, strays) is
/// to remove. A missing `install/` is an empty scan. A link (symbolic link, junction or other
/// reparse point) or a file at `agents`, `agents/<id>` or `install` fails with `InvalidInput`
/// and nothing is listed: `read_dir` would follow it, and the removal would empty a folder
/// outside the data folder.
pub(crate) fn scan(
    data_dir: &Path,
    agent: &AgentId,
    hash: &str,
    platform: Platform,
) -> io::Result<Scan> {
    let root = install_root(data_dir, agent);
    if !write::real_dirs_under(data_dir, &root)? {
        return Ok(Scan::default());
    }
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Scan::default()),
        Err(err) => return Err(err),
    };
    let mut found = Scan::default();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let is_dir = entry.file_type()?.is_dir();
        if is_dir
            && entry.file_name() == hash
            && let Some(receipt) = read_receipt(&path, agent, hash, platform)
        {
            found.keep = Some((path, receipt));
        } else {
            found.remove.push(path);
        }
    }
    found.remove.sort();
    Ok(found)
}

/// Removes everything `scan` marked: folders with what is in them, files and links themselves.
pub(crate) fn remove_stale(scan: &Scan) -> io::Result<()> {
    for path in &scan.remove {
        write::remove_any(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
