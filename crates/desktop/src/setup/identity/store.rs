//! The identity record: `<data_dir>/agents/identity.json`, each agent id to the SHA-256 (hex)
//! of its recorded account, never an email, and whether the agent was signed out through
//! Clauseo since (so a restart keeps the signed-out state, not unknown). An agent that was not
//! is its hash alone, `"codex-acp": "<hex>"`; one that was is
//! `"codex-acp": { "account": "<hex>", "signed_out": true }`. Written whole through
//! `Files::write_atomic`, and not at all when it would not change. A file that is missing
//! records nothing; one that cannot be read or does not parse (an unknown field included) is an
//! error, which the caller takes as unknown for every agent (fail closed).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clauseo_agents::AccountHash;
use clauseo_protocol::AgentId;
use clauseo_settings::Files;
use serde_json::{Map, Value};

use crate::bridge::lock;

/// The record's name, in `<data_dir>/agents/`.
pub(crate) const RECORD: &str = "identity.json";

/// The account field of an agent signed out through Clauseo.
const ACCOUNT: &str = "account";
/// Its signed-out mark.
const SIGNED_OUT: &str = "signed_out";

/// What the record holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Record {
    /// Each agent's recorded account.
    pub(crate) accounts: BTreeMap<AgentId, AccountHash>,
    /// The agents with an account recorded that were signed out through Clauseo since.
    pub(crate) signed_out: BTreeSet<AgentId>,
}

/// Where the record lives and how it is written; none in the tests' memory-only state.
pub(crate) struct Store {
    at: Option<(PathBuf, Arc<dyn Files + Send + Sync>)>,
    /// What the file was last read or written as, so an unchanged record is not written.
    last: Mutex<Option<Record>>,
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

    /// The record: empty when the file is missing; the error's kind when it cannot be read or
    /// does not parse as the module docs say.
    pub(crate) fn load(&self) -> Result<Record, io::ErrorKind> {
        let Some((path, files)) = &self.at else {
            return Ok(Record::default());
        };
        let Some(bytes) = files.read(path).map_err(|error| error.kind())? else {
            *lock(&self.last) = Some(Record::default());
            return Ok(Record::default());
        };
        let parsed = parse(&bytes).ok_or(io::ErrorKind::InvalidData)?;
        *lock(&self.last) = Some(parsed.clone());
        Ok(parsed)
    }

    /// Writes `record` whole, unless it is what the file last held. A signed-out mark with no
    /// account recorded is not written.
    pub(crate) fn save(&self, record: &Record) -> io::Result<()> {
        let Some((path, files)) = &self.at else {
            return Ok(());
        };
        let mut last = lock(&self.last);
        if last.as_ref() == Some(record) {
            return Ok(());
        }
        let text: Map<String, Value> = record
            .accounts
            .iter()
            .map(|(agent, hash)| {
                let hex = Value::String(hash.as_hex().to_owned());
                let value = if record.signed_out.contains(agent) {
                    let mut fields = Map::new();
                    fields.insert(ACCOUNT.to_owned(), hex);
                    fields.insert(SIGNED_OUT.to_owned(), Value::Bool(true));
                    Value::Object(fields)
                } else {
                    hex
                };
                (agent.as_str().to_owned(), value)
            })
            .collect();
        let mut bytes = serde_json::to_vec_pretty(&text).map_err(io::Error::other)?;
        bytes.push(b'\n');
        files.write_atomic(path, &bytes)?;
        *last = Some(record.clone());
        Ok(())
    }
}

/// The record, or `None` when any entry is not an agent id to a hash or to the signed-out
/// form (an `account` hash and `signed_out`, a boolean, and no other field).
fn parse(bytes: &[u8]) -> Option<Record> {
    let text: Map<String, Value> = serde_json::from_slice(bytes).ok()?;
    let mut record = Record::default();
    for (agent, value) in text {
        let agent = AgentId::new(&agent).ok()?;
        let (hex, signed_out) = match &value {
            Value::String(hex) => (hex.as_str(), false),
            Value::Object(fields) => {
                let known = fields.keys().all(|key| key == ACCOUNT || key == SIGNED_OUT);
                let hex = fields.get(ACCOUNT).and_then(Value::as_str);
                let signed_out = match fields.get(SIGNED_OUT) {
                    None => Some(false),
                    Some(mark) => mark.as_bool(),
                };
                (hex.filter(|_| known)?, signed_out?)
            }
            _ => return None,
        };
        if signed_out {
            record.signed_out.insert(agent.clone());
        }
        record.accounts.insert(agent, AccountHash::from_hex(hex)?);
    }
    Some(record)
}
