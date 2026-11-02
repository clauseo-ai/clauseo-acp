//! The hash manifest, `.clauseo-manifest.json`, at the root of each skills folder Clauseo
//! writes: for each skill, its version and the SHA-256 of each of its files.
//!
//! ```json
//! {"skills": {"stetdoc": {"version": "stetdoc 0.9.1", "files": {"SKILL.md": "<hex>"}}}}
//! ```
//!
//! A render rewrites only the entries of the skills it is given; every other entry stays as
//! the file had it.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::error::ProfileError;

/// The manifest's file name.
pub const MANIFEST: &str = ".clauseo-manifest.json";

/// One skill's entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    /// The version the files were written for.
    pub version: String,
    /// Each file's path in the skill's folder (`/`-separated) and its SHA-256, in hex.
    pub files: BTreeMap<String, String>,
}

impl Entry {
    fn from_value(value: &Value) -> Option<Self> {
        let version = value.get("version")?.as_str()?.to_owned();
        let files = value
            .get("files")?
            .as_object()?
            .iter()
            .map(|(path, hash)| Some((path.clone(), hash.as_str()?.to_owned())))
            .collect::<Option<_>>()?;
        Some(Self { version, files })
    }

    fn to_value(&self) -> Value {
        // Keys in sorted order, so the bytes are the same whether or not serde_json's
        // `preserve_order` is on in the build.
        serde_json::json!({"files": self.files, "version": self.version})
    }
}

/// A skills folder's manifest, read whole; entries the profile does not touch stay as values.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Manifest {
    skills: Map<String, Value>,
}

impl Manifest {
    /// The manifest in `root`: empty when there is none, and empty with a warning when it does
    /// not parse (the next render then rewrites every skill it is given).
    pub fn read(root: &Path) -> Result<Self, ProfileError> {
        let path = root.join(MANIFEST);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(ProfileError::io("read", &path, err)),
        };
        let skills = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| value.get("skills")?.as_object().cloned());
        if let Some(skills) = skills {
            return Ok(Self { skills });
        }
        tracing::warn!(path = %path.display(), "a skills manifest does not parse");
        Ok(Self::default())
    }

    /// The entry of `skill`, if it has one that parses.
    pub fn entry(&self, skill: &str) -> Option<Entry> {
        self.skills.get(skill).and_then(Entry::from_value)
    }

    /// Sets the entry of `skill`.
    pub fn set(&mut self, skill: &str, entry: &Entry) {
        self.skills.insert(skill.to_owned(), entry.to_value());
    }

    /// The file's bytes: pretty JSON with keys in order, and a final newline.
    pub fn to_bytes(&self) -> Vec<u8> {
        // Skills sorted by name, whatever order they were read or set in.
        let skills: BTreeMap<&String, &Value> = self.skills.iter().collect();
        let value = serde_json::json!({ "skills": skills });
        let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
        text.push('\n');
        text.into_bytes()
    }
}

/// The SHA-256 of `bytes`, in lower-case hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        let _infallible = write!(out, "{byte:02x}");
    }
    out
}
