//! The ledger file's contents: reading it (with each entry checked against whom it may name),
//! and writing it back.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{Entry, LEDGER_VERSION};
use crate::windows::acl_plan::{LOW_LABEL_SID, OWNER_RIGHTS_SID};
use crate::windows::sids::{Reserved, Sid};

/// Which SIDs a ledger entry may name to be acted on.
#[derive(Debug, Clone)]
pub(super) struct Trust {
    pub(super) trusted: Vec<Sid>,
    pub(super) reserved: Reserved,
}

impl Trust {
    /// Whether `entry` may be acted on: its path is absolute, and it is the Low label naming
    /// the Low level, or an access entry naming one of the trusted SIDs (the pool users and their
    /// group) or a matter-form SID the reserved accounts and domains admit.
    pub(super) fn admits(&self, entry: &Entry) -> bool {
        let sid_ok = if entry.rights.is_label() {
            entry.sid.as_str() == LOW_LABEL_SID
        } else if entry.sid.as_str() == OWNER_RIGHTS_SID {
            entry.rights.is_allow()
        } else {
            self.trusted.contains(&entry.sid)
                || (entry.sid.is_matter_form() && self.reserved.admits(&entry.sid))
        };
        sid_ok && entry.path.is_absolute()
    }
}

/// An entry as read: one this version reads and trusts, with any fields a newer version added
/// (kept so a rewrite doesn't lose them), or one kept as it was and never acted on.
#[derive(Debug, Clone)]
pub(super) enum Slot {
    Known(Known),
    SetAside(Value),
}

/// An entry this version reads, and the fields it doesn't know.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Known {
    #[serde(flatten)]
    pub(super) entry: Entry,
    #[serde(flatten)]
    pub(super) extra: Map<String, Value>,
}

/// The file's contents.
#[derive(Debug, Clone)]
pub(super) struct State {
    pub(super) slots: Vec<Slot>,
    pub(super) next_id: u64,
    pub(super) extra: Map<String, Value>,
}

impl State {
    pub(super) fn empty() -> State {
        State {
            slots: Vec::new(),
            next_id: 1,
            extra: Map::new(),
        }
    }

    pub(super) fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.slots.iter().filter_map(|slot| match slot {
            Slot::Known(known) => Some(&known.entry),
            Slot::SetAside(_) => None,
        })
    }
}

/// Whether `path` exists (a link counts), without following it.
pub(super) fn exists(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// The file at `path`, or an empty ledger when there is none.
pub(super) fn read_state(path: &Path, trust: &Trust) -> io::Result<State> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(State::empty()),
        Err(error) => return Err(error),
    };
    let Value::Object(mut top) = serde_json::from_slice(&bytes).map_err(invalid)? else {
        return Err(invalid("the ACL ledger is not a JSON object"));
    };
    let version = match top.remove("version") {
        None => 1,
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| invalid("the ACL ledger's version is not a whole number"))?,
        Some(_) => return Err(invalid("the ACL ledger's version is not a number")),
    };
    if version > LEDGER_VERSION {
        return Err(invalid("the ACL ledger is from a newer Clauseo"));
    }
    let slots: Vec<Slot> = match top.remove("entries") {
        None => Vec::new(),
        Some(Value::Array(items)) => items.into_iter().map(|v| read_slot(v, trust)).collect(),
        Some(_) => return Err(invalid("the ACL ledger's entries are not a list")),
    };
    let stored_next = top.remove("next_id").and_then(|n| n.as_u64()).unwrap_or(1);
    let highest = slots
        .iter()
        .filter_map(|slot| match slot {
            Slot::Known(known) => Some(known.entry.id.0),
            Slot::SetAside(value) => value.get("id").and_then(Value::as_u64),
        })
        .max()
        .unwrap_or(0);
    let next_id = stored_next.max(highest.saturating_add(1));
    let set_aside = slots
        .iter()
        .filter(|s| matches!(s, Slot::SetAside(_)))
        .count();
    if set_aside > 0 {
        tracing::warn!(set_aside, "the ACL ledger holds entries that are set aside");
    }
    Ok(State {
        slots,
        next_id,
        extra: top,
    })
}

/// One entry as read: known if this version reads it all and trusts it, else set aside.
fn read_slot(value: Value, trust: &Trust) -> Slot {
    match Known::deserialize(&value) {
        Ok(known) if trust.admits(&known.entry) => Slot::Known(known),
        Ok(_) | Err(_) => Slot::SetAside(value),
    }
}

/// The file's bytes: pretty JSON, version first, ending in a newline.
pub(super) fn render(state: &State) -> io::Result<Vec<u8>> {
    let mut top = Map::new();
    top.insert("version".to_owned(), Value::from(LEDGER_VERSION));
    top.insert("next_id".to_owned(), Value::from(state.next_id));
    let mut entries = Vec::with_capacity(state.slots.len());
    for slot in &state.slots {
        entries.push(match slot {
            Slot::Known(known) => serde_json::to_value(known).map_err(invalid)?,
            Slot::SetAside(value) => value.clone(),
        });
    }
    top.insert("entries".to_owned(), Value::Array(entries));
    for (key, value) in &state.extra {
        top.entry(key.clone()).or_insert_with(|| value.clone());
    }
    let mut bytes = serde_json::to_vec_pretty(&Value::Object(top)).map_err(invalid)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
