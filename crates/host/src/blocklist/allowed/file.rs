//! `<data_dir>/blocklist/allowed.json`, the sites the lawyer allowed despite the block list:
//! `{"v":1,"always":{"<host>":<ms>},"matters":{"<matter id>":{"<host>":<ms>}}}`, each host
//! with the time it was allowed. The host's own state, never `settings.json`: a firm's policy
//! is read over the settings, and editing them cannot add an allow. Written whole and
//! atomically (`write::replace_private_file`), as `connectors/hints.json` is.
//!
//! Read once, at the start, and trusted only when its bytes hash to the value in the OS secret
//! store (`integrity.rs`). A file that is missing, does not read, is over [`CAP`] or is of
//! another version counts as no allows (logged): the worst a damaged file does is block the sites
//! again. Past [`MAX_HOSTS`] allows, the rest are ignored (logged).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::{AllowScope, AllowedSite, MatterId, Timestamp};
use serde::{Deserialize, Serialize};

use super::InForce;
use crate::log_file::Cause;

/// The file's name in the block list's folder.
pub(crate) const FILE: &str = "allowed.json";
/// The block list's folder in the data folder, beside the stored list.
const FOLDER: &str = "blocklist";
/// The most of the file read, 4 MiB. A larger file counts as none. [`MAX_HOSTS`] allows at their
/// longest take about 3.0 MiB: each a 253-byte host in quotes, a colon, a 17-character time
/// (`-` and `2^53 - 1`) and a comma (274 bytes), in its own matter's object (a 36-byte id in
/// quotes, `:{`, `},`: 42 more), so 316 × 10,000 bytes, and 32 for the rest.
pub(crate) const CAP: u64 = 4 * 1024 * 1024;
/// The most allows read, `always` and every matter's together; the rest are ignored (logged).
/// An allow past it is refused (`actor/allowed.rs`).
pub(crate) const MAX_HOSTS: usize = 10_000;
/// The version this host writes.
const VERSION: u32 = 1;

/// Every stored allow, each host with when it was allowed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Stored {
    #[serde(default)]
    always: BTreeMap<String, Timestamp>,
    #[serde(default)]
    matters: BTreeMap<MatterId, BTreeMap<String, Timestamp>>,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    v: u32,
    #[serde(flatten)]
    stored: Stored,
}

impl Stored {
    /// Adds `host` at `scope` (for `matter` when `Matter`); an allow already there keeps its
    /// time. True when it was not there.
    pub(crate) fn allow(
        &mut self,
        host: &str,
        scope: AllowScope,
        matter: MatterId,
        at: Timestamp,
    ) -> bool {
        let hosts = match scope {
            AllowScope::Always => &mut self.always,
            AllowScope::Matter => self.matters.entry(matter).or_default(),
        };
        if hosts.contains_key(host) {
            return false;
        }
        hosts.insert(host.to_owned(), at);
        true
    }

    /// Removes `host` at `scope` (for `matter` when `Matter`). True when it was there.
    pub(crate) fn revoke(
        &mut self,
        host: &str,
        scope: AllowScope,
        matter: Option<MatterId>,
    ) -> bool {
        match (scope, matter) {
            (AllowScope::Always, _) => self.always.remove(host).is_some(),
            (AllowScope::Matter, Some(matter)) => {
                let Some(hosts) = self.matters.get_mut(&matter) else {
                    return false;
                };
                let removed = hosts.remove(host).is_some();
                if hosts.is_empty() {
                    self.matters.remove(&matter);
                }
                removed
            }
            (AllowScope::Matter, None) => false,
        }
    }

    /// Whether `host` is allowed at `scope` (for `matter` when `Matter`).
    pub(crate) fn contains(&self, host: &str, scope: AllowScope, matter: Option<MatterId>) -> bool {
        match (scope, matter) {
            (AllowScope::Always, _) => self.always.contains_key(host),
            (AllowScope::Matter, Some(matter)) => self
                .matters
                .get(&matter)
                .is_some_and(|hosts| hosts.contains_key(host)),
            (AllowScope::Matter, None) => false,
        }
    }

    /// How many allows there are, `always` and every matter's together.
    pub(crate) fn count(&self) -> usize {
        self.always.len() + self.matters.values().map(BTreeMap::len).sum::<usize>()
    }

    /// The hosts in force: all of them when `honoured`, none when overrides are off.
    pub(crate) fn in_force(&self, honoured: bool) -> InForce {
        if !honoured {
            return InForce::default();
        }
        InForce {
            always: self.always.keys().cloned().collect(),
            matters: self
                .matters
                .iter()
                .map(|(matter, hosts)| (*matter, hosts.keys().cloned().collect()))
                .collect(),
        }
    }

    /// Every allow as the query lists it: `Always` first, each sorted by host, then the
    /// `Matter` ones by matter and host; each `saved` when `persisted` (what the next start
    /// honours, if known) holds it.
    pub(crate) fn list(&self, persisted: Option<&Stored>) -> Vec<AllowedSite> {
        let saved = |host: &str, scope, matter| {
            persisted.is_some_and(|persisted| persisted.contains(host, scope, matter))
        };
        let always = self.always.iter().map(|(host, at)| AllowedSite {
            host: host.clone(),
            scope: AllowScope::Always,
            matter: None,
            allowed_at: *at,
            saved: saved(host, AllowScope::Always, None),
        });
        let matters = self.matters.iter().flat_map(|(matter, hosts)| {
            hosts.iter().map(|(host, at)| AllowedSite {
                host: host.clone(),
                scope: AllowScope::Matter,
                matter: Some(*matter),
                allowed_at: *at,
                saved: saved(host, AllowScope::Matter, Some(*matter)),
            })
        });
        always.chain(matters).collect()
    }
}

/// Where the file is for `data_dir`.
pub(crate) fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FOLDER).join(FILE)
}

/// Writes `stored` to `path` ([`write_bytes`] of [`encode`]), returning the bytes (tests).
#[cfg(test)]
pub(crate) fn write(path: &Path, stored: &Stored) -> io::Result<Vec<u8>> {
    let bytes = encode(stored)?;
    write_bytes(path, &bytes)?;
    Ok(bytes)
}

/// The file's bytes for `stored`, which the stored hash covers.
pub(crate) fn encode(stored: &Stored) -> io::Result<Vec<u8>> {
    let disk = OnDisk {
        v: VERSION,
        stored: stored.clone(),
    };
    serde_json::to_vec(&disk).map_err(io::Error::other)
}

/// Writes `bytes` ([`encode`]'s) to `path`, whole and atomically, making the folder first, as
/// the stored list is (`blocklist/disk.rs`): an earlier crash's temp files swept, the folder
/// synced after.
pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let folder = path.parent().unwrap_or(path);
    crate::write::create_dir_all(folder)?;
    crate::write::sweep_temp_files(path);
    crate::write::replace_private_file(path, bytes)?;
    crate::write::sync_dir(folder)
}

/// `stored` with at most [`MAX_HOSTS`] allows, `always` first, then by matter; the rest are
/// dropped, logged once with their count.
fn capped(mut stored: Stored) -> Stored {
    let mut left = MAX_HOSTS;
    let mut dropped = 0_usize;
    let mut keep = |hosts: &mut BTreeMap<String, Timestamp>| {
        let kept = hosts.len().min(left);
        dropped += hosts.len() - kept;
        left -= kept;
        while hosts.len() > kept {
            hosts.pop_last();
        }
    };
    keep(&mut stored.always);
    for hosts in stored.matters.values_mut() {
        keep(hosts);
    }
    stored.matters.retain(|_, hosts| !hosts.is_empty());
    if dropped > 0 {
        tracing::warn!(
            dropped,
            max = MAX_HOSTS,
            "blocklist: allowed.json has too many allows"
        );
    }
    stored
}

/// The allows on disk, unchecked (tests); none when the file is missing or unusable.
#[cfg(test)]
pub(crate) fn read(path: &Path) -> Stored {
    match body(path) {
        Body::Read(bytes) => parse(&bytes).unwrap_or_default(),
        Body::Missing | Body::Unusable(_) => Stored::default(),
    }
}

/// What the start-up read of the file found.
pub(crate) enum Body {
    /// No file.
    Missing,
    /// A file that does not read, or is over [`CAP`] (logged); the reason's kind.
    Unusable(&'static str),
    /// The file's bytes.
    Read(Vec<u8>),
}

/// The file's bytes, if it is there and reads within [`CAP`].
pub(crate) fn body(path: &Path) -> Body {
    match crate::blocklist::disk::read_capped(path, CAP) {
        Ok(bytes) => Body::Read(bytes),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Body::Missing,
        Err(err) if err.kind() == io::ErrorKind::InvalidData => {
            tracing::warn!(
                cap = CAP,
                "blocklist: allowed.json is over its size cap; no allows"
            );
            Body::Unusable("over the size cap")
        }
        Err(err) => {
            tracing::warn!(error = %Cause(&err), "blocklist: allowed.json does not read; no allows");
            Body::Unusable("unreadable")
        }
    }
}

/// The allows in `bytes`, a checked file's; `None` when they are unusable (logged).
pub(crate) fn parse(bytes: &[u8]) -> Option<Stored> {
    match serde_json::from_slice::<OnDisk>(bytes) {
        Ok(disk) if disk.v == VERSION => Some(capped(disk.stored)),
        Ok(disk) => {
            tracing::warn!(
                version = disk.v,
                "blocklist: allowed.json is of another version; no allows"
            );
            None
        }
        Err(_unsaid) => {
            tracing::warn!("blocklist: allowed.json does not parse; no allows");
            None
        }
    }
}
