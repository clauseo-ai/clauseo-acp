//! The state file, `<data_dir>/connectors/state.json`: which connectors this install added and
//! how their credential ended. It holds no secret:
//! `{"v":1,"connectors":{"case-law":{"kind":"oauth","ended":null}}}`, where `ended` is `null`,
//! `"signed_out"` or `"key_revoked"`.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::credential::{CredentialKind, CredentialStatus};
use crate::error::StoreError;
use crate::write;

/// The state file's version.
const VERSION: u32 = 1;

/// The state file's contents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) connectors: BTreeMap<String, Entry>,
}

/// One added connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub(crate) kind: CredentialKind,
    #[serde(default)]
    pub(crate) ended: Option<Ended>,
}

/// How a connector's credential ended, while it stays added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Ended {
    /// Signed out, or the refresh token was refused.
    SignedOut,
    /// The API key was refused.
    KeyRevoked,
}

#[derive(Serialize)]
struct FileOut<'a> {
    v: u32,
    connectors: &'a BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
struct FileIn {
    #[serde(default)]
    connectors: BTreeMap<String, serde_json::Value>,
}

impl State {
    /// Reads the state file. A missing file is an empty state. A file that does not parse is an
    /// empty state too, with a warning; an entry that does not parse is left out.
    pub(crate) fn load(path: &Path) -> Result<State, StoreError> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(State::default()),
            Err(err) => return Err(StoreError::StateFile(format!("read: {}", err.kind()))),
        };
        Ok(State::parse(&bytes))
    }

    /// The state in a file's bytes; see [`State::load`].
    pub(crate) fn parse(bytes: &[u8]) -> State {
        let file: FileIn = match serde_json::from_slice(bytes) {
            Ok(file) => file,
            Err(err) => {
                tracing::warn!(category = ?err.classify(), "the connectors' state file does not parse; read as empty");
                return State::default();
            }
        };
        let connectors = file
            .connectors
            .into_iter()
            .filter_map(|(id, entry)| {
                if let Ok(entry) = serde_json::from_value::<Entry>(entry) {
                    Some((id, entry))
                } else {
                    tracing::warn!(connector = %id, "a state file entry does not parse; left out");
                    None
                }
            })
            .collect();
        State { connectors }
    }

    /// The file's bytes.
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(&FileOut {
            v: VERSION,
            connectors: &self.connectors,
        })
        .unwrap_or_default();
        bytes.push(b'\n');
        bytes
    }

    /// Writes the state file atomically (temp file and rename), making its folder.
    pub(crate) fn save(&self, path: &Path) -> Result<(), StoreError> {
        write::replace_atomic(path, &self.to_bytes())
            .map_err(|err| StoreError::StateFile(format!("write: {}", err.kind())))
    }

    /// Marks `id` added with `kind`, its credential live.
    pub(crate) fn add(&mut self, id: &str, kind: CredentialKind) {
        self.connectors
            .insert(id.to_owned(), Entry { kind, ended: None });
    }

    /// Records how `id`'s credential ended; nothing when `id` is not added.
    pub(crate) fn end(&mut self, id: &str, ended: Ended) {
        if let Some(entry) = self.connectors.get_mut(id) {
            entry.ended = Some(ended);
        }
    }

    /// Forgets `id`.
    pub(crate) fn remove(&mut self, id: &str) {
        self.connectors.remove(id);
    }

    /// `id`'s status, given the kind of the keychain item, if there is one: no entry is
    /// `NotAdded`; an entry and an item is `SignedIn`; an entry without an item is
    /// `KeyRevoked` or `SignedOut` from `ended`.
    pub(crate) fn status(&self, id: &str, item: Option<CredentialKind>) -> CredentialStatus {
        let Some(entry) = self.connectors.get(id) else {
            return CredentialStatus::NotAdded;
        };
        match (item, entry.ended) {
            (Some(kind), _) => CredentialStatus::SignedIn(kind),
            (None, Some(Ended::KeyRevoked)) => CredentialStatus::KeyRevoked,
            (None, _) => CredentialStatus::SignedOut,
        }
    }
}

#[cfg(test)]
mod tests;
