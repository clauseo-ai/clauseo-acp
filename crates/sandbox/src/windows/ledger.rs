//! The ACL ledger, `<data_dir>\sandbox\acl-ledger.json`: every permission entry tier B writes,
//! so set-up can remove them and a matter can be forgotten (S15's D4, R6).
//!
//! It is write-ahead. An entry is recorded `pending` and the file replaced before its permission
//! entry is written; once it is written, the entry is marked `applied`. At start, [`reconcile`]
//! keeps each `pending` entry whose permission entry is there (marking it `applied`) and drops
//! the rest. So every permission entry written is listed, and no `applied` entry lacks one.
//!
//! The file is replaced whole, through a temporary file beside it and a rename
//! (`ledger/write.rs`), so a failed write leaves the old file as it was. Nothing here writes
//! unless the entries changed: a launch refused by the gate never reaches the ledger, and a
//! ledger only read is never rewritten.
//!
//! An entry this version can't read (an older shape, a newer kind) is kept as it is, written
//! back unchanged, and never acted on. A file from a newer format version, or one that isn't
//! JSON, is refused with `InvalidData` and left alone. The caller holds one [`Ledger`] per data
//! folder and serialises its calls.
//!
//! [`reconcile`]: Ledger::reconcile

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::acl_plan::{Grant, Inherit, Rights, Sid};
use super::admit::PathKind;

mod write;

/// The ledger's file name in `<data_dir>\sandbox`.
pub const LEDGER_FILE: &str = "acl-ledger.json";

/// The format version this code writes and the newest it reads.
pub const LEDGER_VERSION: u64 = 1;

/// `<data_dir>\sandbox\acl-ledger.json`.
pub fn ledger_path(data_dir: &Path) -> PathBuf {
    data_dir.join("sandbox").join(LEDGER_FILE)
}

/// Whether an entry's permission entry is known to be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    /// Recorded before the permission entry is written; it may or may not be there.
    Pending,
    /// The permission entry was written.
    Applied,
}

/// An entry's number, unique in its ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub u64);

/// One permission entry, listed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Its number.
    pub id: EntryId,
    /// The matter it was written for.
    pub matter: MatterId,
    /// Pending or applied.
    pub state: EntryState,
    /// Where.
    pub path: PathBuf,
    /// Whom.
    pub sid: Sid,
    /// What.
    pub rights: Rights,
    /// How far down.
    pub inherit: Inherit,
    /// Which of the policy's paths.
    pub kind: PathKind,
}

impl Entry {
    /// The permission entry, as planned.
    pub fn grant(&self) -> Grant {
        Grant {
            path: self.path.clone(),
            sid: self.sid.clone(),
            rights: self.rights,
            inherit: self.inherit,
            kind: self.kind,
        }
    }

    /// Whether this entry lists `grant` for `matter`.
    fn lists(&self, matter: MatterId, grant: &Grant) -> bool {
        self.matter == matter
            && self.path == grant.path
            && self.sid == grant.sid
            && self.rights == grant.rights
            && self.inherit == grant.inherit
            && self.kind == grant.kind
    }
}

/// An entry as read: one this version understands, with any fields a newer version added (kept
/// so a rewrite doesn't lose them), or one kept as it was.
#[derive(Debug, Clone)]
enum Slot {
    Known(Known),
    Unknown(Value),
}

/// An entry this version reads, and the fields it doesn't know.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Known {
    #[serde(flatten)]
    entry: Entry,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// What [`Ledger::reconcile`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// Pending entries whose permission entry was there, now applied.
    pub applied: usize,
    /// Pending entries whose permission entry wasn't there, dropped.
    pub dropped: usize,
    /// Pending entries the check failed on, kept pending.
    pub unchecked: usize,
}

/// What [`Ledger::forget_matter`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Forgotten {
    /// Entries whose permission entry was removed, now dropped.
    pub removed: usize,
    /// Entries whose removal failed, kept for the next try.
    pub kept: usize,
}

/// The ledger, read into memory. Every change replaces the file before it returns.
#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
    slots: Vec<Slot>,
    next_id: u64,
    extra: Map<String, Value>,
}

impl Ledger {
    /// Reads the ledger at `path` (see [`ledger_path`]). A missing file is an empty ledger, and
    /// nothing is written until an entry is recorded. A newer format version or a file that
    /// isn't a JSON object is `InvalidData`; entries this version can't read are kept as they
    /// are.
    pub fn open(path: impl Into<PathBuf>) -> io::Result<Ledger> {
        let path = path.into();
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(Ledger {
                    path,
                    slots: Vec::new(),
                    next_id: 1,
                    extra: Map::new(),
                });
            }
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
            Some(Value::Array(items)) => items.into_iter().map(read_slot).collect(),
            Some(_) => return Err(invalid("the ACL ledger's entries are not a list")),
        };
        let stored_next = top.remove("next_id").and_then(|n| n.as_u64()).unwrap_or(1);
        let highest = slots
            .iter()
            .filter_map(|slot| match slot {
                Slot::Known(Known { entry, .. }) => Some(entry.id.0),
                Slot::Unknown(value) => value.get("id").and_then(Value::as_u64),
            })
            .max()
            .unwrap_or(0);
        let next_id = stored_next.max(highest.saturating_add(1));
        let unknown = slots
            .iter()
            .filter(|s| matches!(s, Slot::Unknown(_)))
            .count();
        if unknown > 0 {
            tracing::warn!(
                unknown,
                "the ACL ledger holds entries this version can't read"
            );
        }
        Ok(Ledger {
            path,
            slots,
            next_id,
            extra: top,
        })
    }

    /// The file this ledger replaces.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The entries this version can read, in file order.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.slots.iter().filter_map(|slot| match slot {
            Slot::Known(Known { entry, .. }) => Some(entry),
            Slot::Unknown(_) => None,
        })
    }

    /// How many entries this version can't read; they are kept and never acted on.
    pub fn unknown_entries(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| matches!(slot, Slot::Unknown(_)))
            .count()
    }

    /// `matter`'s entries, for removal.
    pub fn matter_entries(&self, matter: MatterId) -> Vec<Entry> {
        self.entries()
            .filter(|entry| entry.matter == matter)
            .cloned()
            .collect()
    }

    /// The write-ahead step: lists each of `grants` for `matter` as `pending`, then replaces the
    /// file, before any permission entry is written. A grant already listed for the matter,
    /// pending or applied, keeps its entry. Returns each grant's entry number, in order. The
    /// file is untouched when every grant was listed already, or when the write fails (the
    /// ledger in memory is then as it was).
    pub fn record_pending(
        &mut self,
        matter: MatterId,
        grants: &[Grant],
    ) -> io::Result<Vec<EntryId>> {
        let mut slots = self.slots.clone();
        let mut next_id = self.next_id;
        let mut ids = Vec::with_capacity(grants.len());
        for grant in grants {
            let listed = slots.iter().find_map(|slot| match slot {
                Slot::Known(known) if known.entry.lists(matter, grant) => Some(known.entry.id),
                Slot::Known(_) | Slot::Unknown(_) => None,
            });
            if let Some(id) = listed {
                ids.push(id);
                continue;
            }
            let id = EntryId(next_id);
            next_id = next_id
                .checked_add(1)
                .ok_or_else(|| invalid("the ACL ledger ran out of entry numbers"))?;
            let entry = Entry {
                id,
                matter,
                state: EntryState::Pending,
                path: grant.path.clone(),
                sid: grant.sid.clone(),
                rights: grant.rights,
                inherit: grant.inherit,
                kind: grant.kind,
            };
            slots.push(Slot::Known(Known {
                entry,
                extra: Map::new(),
            }));
            ids.push(id);
        }
        if next_id != self.next_id {
            self.commit(slots, next_id)?;
        }
        Ok(ids)
    }

    /// Marks the entries `ids` `applied`, once their permission entries are written. Numbers
    /// not listed are ignored.
    pub fn mark_applied(&mut self, ids: &[EntryId]) -> io::Result<()> {
        let mut changed = false;
        let mut slots = self.slots.clone();
        for slot in &mut slots {
            if let Slot::Known(Known { entry, .. }) = slot
                && ids.contains(&entry.id)
                && entry.state == EntryState::Pending
            {
                entry.state = EntryState::Applied;
                changed = true;
            }
        }
        if changed {
            self.commit(slots, self.next_id)?;
        }
        Ok(())
    }

    /// Drops the entries `ids`, once their permission entries are known to be gone (a launch's
    /// rollback). Numbers not listed are ignored.
    pub fn drop_entries(&mut self, ids: &[EntryId]) -> io::Result<()> {
        let before = self.slots.len();
        let slots: Vec<Slot> = self
            .slots
            .iter()
            .filter(
                |slot| !matches!(slot, Slot::Known(Known { entry, .. }) if ids.contains(&entry.id)),
            )
            .cloned()
            .collect();
        if slots.len() != before {
            self.commit(slots, self.next_id)?;
        }
        Ok(())
    }

    /// At start: asks `present` about each `pending` entry. One whose permission entry is there
    /// is marked `applied`; one whose permission entry isn't is dropped; one `present` fails on
    /// stays pending for the next start. Applied entries and unread ones are left as they are.
    /// The file is replaced once, if anything changed.
    pub fn reconcile(
        &mut self,
        mut present: impl FnMut(&Entry) -> io::Result<bool>,
    ) -> io::Result<Reconciled> {
        let mut done = Reconciled::default();
        let mut slots = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            let Slot::Known(known) = slot else {
                slots.push(slot.clone());
                continue;
            };
            let entry = &known.entry;
            if entry.state != EntryState::Pending {
                slots.push(slot.clone());
                continue;
            }
            match present(entry) {
                Ok(true) => {
                    let mut known = known.clone();
                    known.entry.state = EntryState::Applied;
                    slots.push(Slot::Known(known));
                    done.applied += 1;
                }
                Ok(false) => done.dropped += 1,
                Err(error) => {
                    // The kinds alone: the error's text may hold the path.
                    let (kind, error) = (entry.kind, error.kind());
                    tracing::warn!(%kind, %error, "a pending ACL entry wasn't checked");
                    slots.push(slot.clone());
                    done.unchecked += 1;
                }
            }
        }
        if done.applied + done.dropped > 0 {
            self.commit(slots, self.next_id)?;
        }
        Ok(done)
    }

    /// Forgets `matter`: hands each of its entries to `remove`, which removes the permission
    /// entry (an entry that is already gone counts as removed). Entries removed are dropped;
    /// entries `remove` failed on are kept for the next try. Other matters' entries are left as
    /// they are. The file is replaced once, if anything was dropped.
    pub fn forget_matter(
        &mut self,
        matter: MatterId,
        mut remove: impl FnMut(&Entry) -> io::Result<()>,
    ) -> io::Result<Forgotten> {
        let mut done = Forgotten::default();
        let mut slots = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            match slot {
                Slot::Known(Known { entry, .. }) if entry.matter == matter => match remove(entry) {
                    Ok(()) => done.removed += 1,
                    Err(error) => {
                        let (kind, error) = (entry.kind, error.kind());
                        tracing::warn!(%kind, %error, "an ACL entry wasn't removed");
                        slots.push(slot.clone());
                        done.kept += 1;
                    }
                },
                Slot::Known(_) | Slot::Unknown(_) => slots.push(slot.clone()),
            }
        }
        if done.removed > 0 {
            self.commit(slots, self.next_id)?;
        }
        Ok(done)
    }

    /// Replaces the file with `slots`, then takes them; on a failed write the ledger in memory
    /// stays as it was, matching the file.
    fn commit(&mut self, slots: Vec<Slot>, next_id: u64) -> io::Result<()> {
        let bytes = render(&slots, next_id, &self.extra)?;
        write::replace(&self.path, &bytes)?;
        self.slots = slots;
        self.next_id = next_id;
        Ok(())
    }
}

/// One entry as read: known if this version can read it all, else kept as it is.
fn read_slot(value: Value) -> Slot {
    match Known::deserialize(&value) {
        Ok(known) => Slot::Known(known),
        Err(_) => Slot::Unknown(value),
    }
}

/// The file's bytes: pretty JSON, version first, ending in a newline.
fn render(slots: &[Slot], next_id: u64, extra: &Map<String, Value>) -> io::Result<Vec<u8>> {
    let mut top = Map::new();
    top.insert("version".to_owned(), Value::from(LEDGER_VERSION));
    top.insert("next_id".to_owned(), Value::from(next_id));
    let mut entries = Vec::with_capacity(slots.len());
    for slot in slots {
        entries.push(match slot {
            Slot::Known(known) => serde_json::to_value(known).map_err(invalid)?,
            Slot::Unknown(value) => value.clone(),
        });
    }
    top.insert("entries".to_owned(), Value::Array(entries));
    for (key, value) in extra {
        top.entry(key.clone()).or_insert_with(|| value.clone());
    }
    let mut bytes = serde_json::to_vec_pretty(&Value::Object(top)).map_err(invalid)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests;
