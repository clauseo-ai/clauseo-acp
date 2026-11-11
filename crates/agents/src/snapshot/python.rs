//! The Python runtime this build downloads beside Node: Clauseo's own archive per platform (a
//! `.tar.gz` CI builds from python-build-standalone and the document libraries), its URL, size,
//! unpacked size and SHA-256 compiled in. A platform with no archive has no Python.

use crate::integrity::Expected;
use crate::platform::{Arch, Os, Platform};

/// The Python runtime: its version and one archive per platform that has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonEntry {
    version: String,
    archives: Vec<PythonArchive>,
    hosts: Vec<String>,
}

impl PythonEntry {
    /// Python `version` (`3.13.18`) with `archives`; none means no Python anywhere.
    pub fn new(version: &str, archives: Vec<PythonArchive>) -> PythonEntry {
        PythonEntry {
            version: version.to_owned(),
            archives,
            hosts: Vec::new(),
        }
    }

    /// The same, with `hosts`: the hosts its download contacts, for "Ask your IT team".
    #[must_use]
    pub fn with_hosts(mut self, hosts: Vec<String>) -> PythonEntry {
        self.hosts = hosts;
        self
    }

    /// The hosts its download contacts (the archive's and its redirect's); none unless given.
    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }

    /// No Python on any platform: what a snapshot has unless given one.
    pub fn none() -> PythonEntry {
        PythonEntry::new("", Vec::new())
    }

    /// The version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The archive for `platform`'s OS and CPU, if this build has one.
    pub fn archive(&self, platform: Platform) -> Option<&PythonArchive> {
        self.archives
            .iter()
            .find(|archive| archive.os == platform.os && archive.arch == platform.arch)
    }

    /// Every archive.
    pub fn archives(&self) -> &[PythonArchive] {
        &self.archives
    }
}

/// One Python archive: always a `.tar.gz`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonArchive {
    os: Os,
    arch: Arch,
    name: String,
    url: String,
    size: u64,
    unpacked: u64,
    sha256: Expected,
}

impl PythonArchive {
    /// An archive, or `None` when `sha256_hex` is not 64 hex digits, or `name` is not a plain
    /// file name ending in `.tar.gz` (its stem names the runtime's folder).
    pub fn new(
        os: Os,
        arch: Arch,
        name: &str,
        url: &str,
        size: u64,
        unpacked: u64,
        sha256_hex: &str,
    ) -> Option<PythonArchive> {
        let stem = name.strip_suffix(".tar.gz")?;
        let plain = !stem.is_empty()
            && !stem.starts_with('.')
            && !name.contains(['/', '\\', ':'])
            && !name.contains("..");
        if !plain {
            return None;
        }
        Some(PythonArchive {
            os,
            arch,
            name: name.to_owned(),
            url: url.to_owned(),
            size,
            unpacked,
            sha256: Expected::sha256_hex(sha256_hex)?,
        })
    }

    /// Its OS.
    pub fn os(&self) -> Os {
        self.os
    }

    /// Its CPU.
    pub fn arch(&self) -> Arch {
        self.arch
    }

    /// Its file name, `clauseo-python-<version>-c<build>-<os>-<arch>.tar.gz`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its name without `.tar.gz`: the runtime folder's name under `agents/runtime/`.
    pub fn stem(&self) -> &str {
        self.name.strip_suffix(".tar.gz").unwrap_or(&self.name)
    }

    /// Where it is downloaded from.
    pub fn url(&self) -> &str {
        &self.url
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

    /// Its SHA-256, for the download's check.
    pub(crate) fn expected(&self) -> &Expected {
        &self.sha256
    }
}
