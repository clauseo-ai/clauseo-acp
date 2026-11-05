//! The snapshot: the agents and the Node this build installs, with G16's pins embedded, each
//! agent's derived pair, the sizes measured for each platform and Node's archives.

use std::fmt;
use std::sync::{Arc, OnceLock};

use clauseo_protocol::AgentId;

use crate::integrity::{Expected, short_hash};
use crate::pins::{self, AgentPins, LockedPackage, PinsError};
use crate::platform::{Arch, Os, Platform};

mod embedded;

#[cfg(test)]
use embedded::SIZES_STAMP;
use embedded::{PINS_LOCK, PINS_PACKAGE_JSON, build_embedded, embedded_node};

/// npm's registry: every embedded `resolved` URL starts with it.
pub const NPM_REGISTRY: &str = "https://registry.npmjs.org/";

/// Folder names under `<data_dir>/agents` that are not agents.
pub(crate) const RESERVED_IDS: [&str; 4] = ["runtime", "downloads", "npm-cache", "signin"];

/// One agent the snapshot installs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentEntry {
    /// Its settings id, also its folder under `<data_dir>/agents`.
    pub id: AgentId,
    /// The name the window shows.
    pub display_name: String,
    /// Its root npm package, a dependency in the pins' `package.json`.
    pub root: String,
    /// The adapter's script inside the root package, `/`-separated.
    pub bin: String,
    /// The hosts its install contacts, for "Ask your IT team".
    pub install_hosts: Vec<String>,
    /// The hosts its sign-in contacts.
    pub signin_hosts: Vec<String>,
    /// Its download and unpacked size per platform.
    pub sizes: Vec<AgentSizes>,
}

impl AgentEntry {
    /// An agent with no hosts and no sizes yet.
    pub fn new(id: AgentId, display_name: &str, root: &str, bin: &str) -> AgentEntry {
        AgentEntry {
            id,
            display_name: display_name.to_owned(),
            root: root.to_owned(),
            bin: bin.to_owned(),
            install_hosts: Vec::new(),
            signin_hosts: Vec::new(),
            sizes: Vec::new(),
        }
    }

    /// The sizes row for `platform`'s OS and CPU, if there is one.
    pub fn sizes_for(&self, platform: Platform) -> Option<&AgentSizes> {
        self.sizes
            .iter()
            .find(|row| row.os == platform.os && row.arch == platform.arch)
    }
}

/// One agent's packages for one OS and CPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentSizes {
    /// The OS.
    pub os: Os,
    /// The CPU.
    pub arch: Arch,
    /// The tarballs' bytes.
    pub download_bytes: u64,
    /// The bytes they unpack to.
    pub unpacked_bytes: u64,
}

/// The Node this build downloads when the user has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeEntry {
    version: String,
    base: String,
    archives: Vec<NodeArchive>,
}

impl NodeEntry {
    /// Node `version` (`24.22.0`), downloaded from `base` (ending in `/`), with `archives`.
    pub fn new(version: &str, base: &str, archives: Vec<NodeArchive>) -> NodeEntry {
        NodeEntry {
            version: version.to_owned(),
            base: base.to_owned(),
            archives,
        }
    }

    /// The version, without a `v`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The download folder's URL.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// The archive for `platform`'s OS and CPU.
    pub fn archive(&self, platform: Platform) -> Option<&NodeArchive> {
        self.archives
            .iter()
            .find(|archive| archive.os == platform.os && archive.arch == platform.arch)
    }

    /// The archive's URL.
    pub fn url(&self, archive: &NodeArchive) -> String {
        format!("{}{}", self.base, archive.name)
    }
}

/// One Node archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeArchive {
    os: Os,
    arch: Arch,
    name: String,
    size: u64,
    unpacked: u64,
    sha256: Expected,
}

impl NodeArchive {
    /// An archive, or `None` when `sha256_hex` is not 64 hex digits.
    pub fn new(
        os: Os,
        arch: Arch,
        name: &str,
        size: u64,
        unpacked: u64,
        sha256_hex: &str,
    ) -> Option<NodeArchive> {
        Some(NodeArchive {
            os,
            arch,
            name: name.to_owned(),
            size,
            unpacked,
            sha256: Expected::sha256_hex(sha256_hex)?,
        })
    }

    /// Its file name, `node-v<version>-<os>-<arch>.tar.gz` or `.zip`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// The bytes it unpacks to.
    pub fn unpacked_bytes(&self) -> u64 {
        self.unpacked
    }

    /// Its SHA-256, lowercase hex.
    pub fn sha256_hex(&self) -> String {
        self.sha256.hex()
    }

    /// Whether it is a zip (Windows) rather than a `.tar.gz`.
    pub fn is_zip(&self) -> bool {
        std::path::Path::new(&self.name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    }

    /// Its SHA-256, for the download's check.
    pub(crate) fn expected(&self) -> &Expected {
        &self.sha256
    }
}

/// Why [`Snapshot::new`] refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SnapshotError {
    /// An agent id is one of the folder names under `agents/` that are not agents.
    #[error("the agent id {0} is reserved")]
    ReservedId(String),
    /// Two agents have the same id.
    #[error("the agent id {0} is given twice")]
    DuplicateId(String),
    /// An agent's pins did not derive.
    #[error("{agent}'s pins: {source}")]
    Pins {
        /// The agent.
        agent: String,
        /// Why.
        source: PinsError,
    },
}

/// The agents and Node this build installs. Cheap to clone.
#[derive(Clone)]
pub struct Snapshot {
    inner: Arc<Inner>,
}

#[derive(PartialEq, Eq)]
struct Inner {
    package_json: String,
    lock: String,
    registry: String,
    agents: Vec<(AgentEntry, AgentPins)>,
    node: NodeEntry,
}

impl PartialEq for Snapshot {
    fn eq(&self, other: &Snapshot) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner) || *self.inner == *other.inner
    }
}

impl Eq for Snapshot {}

impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<&str> = self.agents().map(|entry| entry.id.as_str()).collect();
        f.debug_struct("Snapshot")
            .field("registry", &self.inner.registry)
            .field("agents", &ids)
            .field("node", &self.inner.node.version)
            .finish_non_exhaustive()
    }
}

impl Snapshot {
    /// The snapshot embedded in this build. Built once; a test checks that it builds, so the
    /// empty fallback below never ships.
    pub fn embedded() -> Snapshot {
        static EMBEDDED: OnceLock<Snapshot> = OnceLock::new();
        EMBEDDED
            .get_or_init(|| {
                build_embedded().unwrap_or_else(|_never| Snapshot {
                    inner: Arc::new(Inner {
                        package_json: PINS_PACKAGE_JSON.to_owned(),
                        lock: PINS_LOCK.to_owned(),
                        registry: NPM_REGISTRY.to_owned(),
                        agents: Vec::new(),
                        node: embedded_node(),
                    }),
                })
            })
            .clone()
    }

    /// A snapshot from pins text, a registry base, Node and agents: tests build fixtures with it.
    /// Refuses a reserved or repeated id, a root that is not in `package_json`, a missing
    /// dependency, a `resolved` outside `registry` and an integrity that is not SHA-512.
    pub fn new(
        package_json: &str,
        lock: &str,
        registry: &str,
        node: NodeEntry,
        agents: Vec<AgentEntry>,
    ) -> Result<Snapshot, SnapshotError> {
        let mut derived: Vec<(AgentEntry, AgentPins)> = Vec::with_capacity(agents.len());
        for entry in agents {
            let id = entry.id.as_str().to_owned();
            if RESERVED_IDS.contains(&id.as_str()) {
                return Err(SnapshotError::ReservedId(id));
            }
            if derived.iter().any(|(seen, _)| seen.id == entry.id) {
                return Err(SnapshotError::DuplicateId(id));
            }
            let pins = pins::derive(package_json, lock, &entry.root, registry)
                .map_err(|source| SnapshotError::Pins { agent: id, source })?;
            derived.push((entry, pins));
        }
        Ok(Snapshot {
            inner: Arc::new(Inner {
                package_json: package_json.to_owned(),
                lock: lock.to_owned(),
                registry: registry.to_owned(),
                agents: derived,
                node,
            }),
        })
    }

    /// The pins' `package.json` text.
    pub fn pins_package_json(&self) -> &str {
        &self.inner.package_json
    }

    /// The pins' lock text.
    pub fn pins_lock(&self) -> &str {
        &self.inner.lock
    }

    /// The registry base every `resolved` URL starts with.
    pub fn registry(&self) -> &str {
        &self.inner.registry
    }

    /// The agent `id`.
    pub fn agent(&self, id: &AgentId) -> Option<&AgentEntry> {
        self.find(id).map(|(entry, _)| entry)
    }

    /// Every agent, in the snapshot's order.
    pub fn agents(&self) -> impl Iterator<Item = &AgentEntry> {
        self.inner.agents.iter().map(|(entry, _)| entry)
    }

    /// The agent's derived `package.json` and lock.
    pub fn pins(&self, id: &AgentId) -> Option<&AgentPins> {
        self.find(id).map(|(_, pins)| pins)
    }

    /// The install folder's `<hash>` for `id` on `platform`: the first 16 hex digits of the
    /// SHA-256 over the id, the derived lock's bytes and the platform (`linux-x64-glibc`), each
    /// prefixed by its length.
    pub fn install_hash(&self, id: &AgentId, platform: Platform) -> Option<String> {
        let pins = self.pins(id)?;
        let platform = platform.to_string();
        Some(short_hash(&[
            id.as_str().as_bytes(),
            pins.lock().as_bytes(),
            platform.as_bytes(),
        ]))
    }

    /// Bytes an install of `id` on `platform` downloads: the agent's packages (its sizes row;
    /// none for a platform with no row) plus Node's archive when `node_needed`.
    pub fn download_bytes(
        &self,
        id: &AgentId,
        platform: Platform,
        node_needed: bool,
    ) -> Option<u64> {
        let row = self.agent(id)?.sizes_for(platform);
        let agent = row.map_or(0, |row| row.download_bytes);
        let node = self.node_part(platform, node_needed, |archive| archive.size);
        Some(agent.saturating_add(node))
    }

    /// The free space an install needs: twice the download (the tarballs and the npm cache)
    /// plus the unpacked size, plus Node's archive and unpacked size when `node_needed`, plus
    /// 10 % (rounded up).
    pub fn need_bytes(&self, id: &AgentId, platform: Platform, node_needed: bool) -> Option<u64> {
        let row = self.agent(id)?.sizes_for(platform);
        let agent = row.map_or(0, |row| {
            row.download_bytes
                .saturating_mul(2)
                .saturating_add(row.unpacked_bytes)
        });
        let node = self.node_part(platform, node_needed, |archive| {
            archive.size.saturating_add(archive.unpacked)
        });
        let base = agent.saturating_add(node);
        Some(base.saturating_add(base.div_ceil(10)))
    }

    /// Node's entry.
    pub fn node(&self) -> &NodeEntry {
        &self.inner.node
    }

    /// Node's archive for `platform`.
    pub fn node_archive(&self, platform: Platform) -> Option<&NodeArchive> {
        self.inner.node.archive(platform)
    }

    /// The derived lock's entries npm installs for `id` on `platform`.
    pub(crate) fn packages(&self, id: &AgentId, platform: Platform) -> Vec<&LockedPackage> {
        self.pins(id)
            .map(|pins| pins.packages_for(platform))
            .unwrap_or_default()
    }

    fn find(&self, id: &AgentId) -> Option<&(AgentEntry, AgentPins)> {
        self.inner.agents.iter().find(|(entry, _)| entry.id == *id)
    }

    fn node_part(&self, platform: Platform, needed: bool, f: impl Fn(&NodeArchive) -> u64) -> u64 {
        if needed {
            self.node_archive(platform).map_or(0, f)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests;
