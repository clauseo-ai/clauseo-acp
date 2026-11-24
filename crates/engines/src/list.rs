//! `engines.json`'s shape, `clauseo-engines/1`, and the verified list the caller gets.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Error, FORMAT, MAX_SEQ};

/// The top-level fields of `clauseo-engines/1`, every one required.
const FIELDS: [&str; 7] = [
    "format",
    "seq",
    "generated",
    "apps",
    "package_json",
    "package_lock",
    "sizes",
];

/// The rows `sizes` has: 2 agents × 3 OSes × 2 architectures (Linux's are glibc's; musl uses
/// them too, as the app's snapshot does).
const SIZE_ROWS: usize = 12;

/// One of the two engines, as `sizes` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Agent {
    /// `@agentclientprotocol/claude-agent-acp`.
    #[serde(rename = "claude-acp")]
    ClaudeAcp,
    /// `@agentclientprotocol/codex-acp`.
    #[serde(rename = "codex-acp")]
    CodexAcp,
}

/// An operating system, as Node's `process.platform` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    /// macOS.
    Darwin,
    /// Windows.
    Win32,
    /// Linux.
    Linux,
}

/// A CPU architecture, as Node's `process.arch` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    /// 64-bit ARM.
    Arm64,
    /// x86-64.
    X64,
}

/// One engine's download and unpacked bytes on one platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    /// The engine.
    pub agent: Agent,
    /// The OS.
    pub os: Os,
    /// The architecture.
    pub arch: Arch,
    /// Bytes downloaded from the registry.
    pub download: u64,
    /// Bytes on disk once installed.
    pub unpacked: u64,
}

/// The file as written; `deny_unknown_fields` backs up the field check in [`parse`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    format: String,
    seq: u64,
    generated: String,
    apps: Vec<String>,
    package_json: Value,
    package_lock: Value,
    sizes: Vec<Size>,
}

/// A list that verified and passed every rule (see the crate docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineList {
    json: Vec<u8>,
    seq: u64,
    generated: String,
    apps: Vec<String>,
    package_json: Value,
    package_lock: Value,
    sizes: Vec<Size>,
}

impl EngineList {
    /// The list's sequence number: each published list's is above the last one's.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// When it was made, `YYYY-MM-DDTHH:MM:SSZ` (its shape was checked, not its time).
    pub fn generated(&self) -> &str {
        &self.generated
    }

    /// The app versions it was tested with, one of them the running app's.
    pub fn apps(&self) -> &[String] {
        &self.apps
    }

    /// The `package.json` to install from.
    pub fn package_json(&self) -> &Value {
        &self.package_json
    }

    /// The `package-lock.json` to install from (`npm ci`).
    pub fn package_lock(&self) -> &Value {
        &self.package_lock
    }

    /// The app's free-space input: each engine's download and unpacked bytes on each platform,
    /// one row for every (agent, os, arch).
    pub fn sizes(&self) -> &[Size] {
        &self.sizes
    }

    /// The exact `engines.json` bytes that verified, for storage only (so the stored file is
    /// the signed one). Install from [`package_json`](Self::package_json) and
    /// [`package_lock`](Self::package_lock), re-serialised; never hand these bytes to another
    /// parser to install from: a key repeated in a JSON object is last-wins here, and another
    /// parser might read it otherwise.
    pub fn json(&self) -> &[u8] {
        &self.json
    }
}

/// Parses `json` and checks its shape: fields, format, seq, generated, apps and sizes.
pub(crate) fn parse(json: &[u8]) -> Result<EngineList, Error> {
    let object: Map<String, Value> = serde_json::from_slice(json).ok().ok_or(Error::Json)?;
    if object.keys().any(|key| !FIELDS.contains(&key.as_str())) {
        return Err(Error::UnknownField);
    }
    let raw: Raw = serde_json::from_value(Value::Object(object))
        .ok()
        .ok_or(Error::Json)?;
    if raw.format != FORMAT {
        return Err(Error::Format);
    }
    if raw.seq == 0 || raw.seq > MAX_SEQ {
        return Err(Error::Seq);
    }
    if !generated_is_rfc3339(&raw.generated) {
        return Err(Error::Generated);
    }
    if raw.apps.is_empty() || raw.apps.iter().any(String::is_empty) {
        return Err(Error::NoApps);
    }
    let mut seen = BTreeSet::new();
    for size in &raw.sizes {
        if size.download == 0 || size.unpacked == 0 {
            return Err(Error::Size);
        }
        if !seen.insert((size.agent, size.os, size.arch)) {
            return Err(Error::DuplicateSize);
        }
    }
    // With no repeats, 12 rows are every agent × OS × architecture the enums name.
    if seen.len() != SIZE_ROWS {
        return Err(Error::SizesIncomplete);
    }
    Ok(EngineList {
        json: json.to_vec(),
        seq: raw.seq,
        generated: raw.generated,
        apps: raw.apps,
        package_json: raw.package_json,
        package_lock: raw.package_lock,
        sizes: raw.sizes,
    })
}

/// `YYYY-MM-DDTHH:MM:SSZ`: UTC, whole seconds, month 1–12, day 1–31, hour < 24, minute and
/// second < 60. The day is not checked against the month.
fn generated_is_rfc3339(text: &str) -> bool {
    let bytes = text.as_bytes();
    let [
        y0,
        y1,
        y2,
        y3,
        b'-',
        m0,
        m1,
        b'-',
        d0,
        d1,
        b'T',
        h0,
        h1,
        b':',
        n0,
        n1,
        b':',
        s0,
        s1,
        b'Z',
    ] = bytes
    else {
        return false;
    };
    let digits = [y0, y1, y2, y3, m0, m1, d0, d1, h0, h1, n0, n1, s0, s1];
    if !digits.iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let two = |a: &u8, b: &u8| (a - b'0') * 10 + (b - b'0');
    let (month, day) = (two(m0, m1), two(d0, d1));
    (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && two(h0, h1) < 24
        && two(n0, n1) < 60
        && two(s0, s1) < 60
}
