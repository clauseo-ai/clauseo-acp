//! The identity record: `<data_dir>/agents/identity.json`, each agent id to the SHA-256 (hex)
//! of its recorded account, never an email. Written whole through `Files::write_atomic`, and
//! not at all when it would not change. A file that is missing records nothing; one that
//! cannot be read or does not parse is an error, which the caller takes as unknown for every
//! agent (fail closed).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_agents::AccountHash;
use clauseo_protocol::AgentId;
use clauseo_settings::Files;

use crate::bridge::lock;

/// The record's name, in `<data_dir>/agents/`.
pub(crate) const RECORD: &str = "identity.json";

/// Where the record lives and how it is written; none in the tests' memory-only state.
pub(crate) struct Store {
    at: Option<(PathBuf, Arc<dyn Files + Send + Sync>)>,
    /// What the file was last read or written as, so an unchanged record is not written.
    last: Mutex<Option<BTreeMap<AgentId, AccountHash>>>,
}

impl Store {
    /// No file: every load is empty and every save does nothing.
    pub(crate) fn none() -> Self {
        Self {
            at: None,
            last: Mutex::new(None),
        }
    }

    /// The record in `<data_dir>/agents/`, through `files`.
    pub(crate) fn at(data_dir: &Path, files: Arc<dyn Files + Send + Sync>) -> Self {
        Self {
            at: Some((data_dir.join("agents").join(RECORD), files)),
            last: Mutex::new(None),
        }
    }

    /// Each agent's recorded account: empty when the file is missing; the error's kind when it
    /// cannot be read, is not JSON of ids to hashes, or holds a hash that is not one.
    pub(crate) fn load(&self) -> Result<BTreeMap<AgentId, AccountHash>, io::ErrorKind> {
        let Some((path, files)) = &self.at else {
            return Ok(BTreeMap::new());
        };
        let Some(bytes) = files.read(path).map_err(|error| error.kind())? else {
            *lock(&self.last) = Some(BTreeMap::new());
            return Ok(BTreeMap::new());
        };
        let parsed = parse(&bytes).ok_or(io::ErrorKind::InvalidData)?;
        *lock(&self.last) = Some(parsed.clone());
        Ok(parsed)
    }

    /// Writes `recorded` whole, unless it is what the file last held.
    pub(crate) fn save(&self, recorded: &BTreeMap<AgentId, AccountHash>) -> io::Result<()> {
        let Some((path, files)) = &self.at else {
            return Ok(());
        };
        let mut last = lock(&self.last);
        if last.as_ref() == Some(recorded) {
            return Ok(());
        }
        let text: BTreeMap<&str, &str> = recorded
            .iter()
            .map(|(agent, hash)| (agent.as_str(), hash.as_hex()))
            .collect();
        let mut bytes = serde_json::to_vec_pretty(&text).map_err(io::Error::other)?;
        bytes.push(b'\n');
        files.write_atomic(path, &bytes)?;
        *last = Some(recorded.clone());
        Ok(())
    }
}

/// The record's ids and hashes, or `None` when any entry is not an agent id to a hash.
fn parse(bytes: &[u8]) -> Option<BTreeMap<AgentId, AccountHash>> {
    let text: BTreeMap<String, String> = serde_json::from_slice(bytes).ok()?;
    text.into_iter()
        .map(|(agent, hash)| Some((AgentId::new(&agent).ok()?, AccountHash::from_hex(&hash)?)))
        .collect()
}
