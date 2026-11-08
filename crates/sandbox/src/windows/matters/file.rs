//! `matters.json`'s format. Version 3 is written: one row per matter, `{ sid?, pool_user?,
//! released_at?, held_before? }`, and `needs_reset`, the pool users released from a matter whose
//! profile and hive the elevated set-up hasn't reset yet. Version 2 (the rows alone) is read with
//! every user a row released, and no row holds, needing a reset, as nothing reset them; version 1
//! (matter id to SID) as rows with only a SID. Both are upgraded on the next write. Any other
//! version is refused, so a version 2 reader refuses a file with resets owed rather than drop
//! them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::path::Path;

use clauseo_protocol::MatterId;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use super::super::sids::{Reserved, Sid};
use super::super::write;
use crate::SetupError;

/// The version `matters.json` is written at.
pub(super) const MATTERS_VERSION: u32 = 3;
/// The version before the resets owed: the rows alone.
const MATTERS_V2: u32 = 2;
/// The version before pool users: matter id to SID.
const MATTERS_V1: u32 = 1;

/// One matter's row in `matters.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatterRow {
    /// The matter's own SID, the restricting SID ([`super::MatterSids::sid_for`]); none until
    /// it is made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sid: Option<Sid>,
    /// The pool user the matter holds, or held last when `released_at` is set
    /// ([`super::MatterSids::assign`]); none until one is assigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool_user: Option<Sid>,
    /// When the matter gave its pool user back, in seconds since the Unix epoch
    /// ([`super::MatterSids::release`]); none while it holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub released_at: Option<u64>,
    /// The pool users the matter held before `pool_user`, oldest first: each a released one
    /// it gave up when assigned again. Kept so the open-session check still finds them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held_before: Vec<Sid>,
}

impl MatterRow {
    /// The pool user the matter holds now: its `pool_user` unless released.
    pub fn holds(&self) -> Option<&Sid> {
        self.pool_user
            .as_ref()
            .filter(|_| self.released_at.is_none())
    }

    /// Whether the row names `user`, now or before: its `pool_user` or in `held_before`.
    pub fn named(&self, user: &Sid) -> bool {
        self.pool_user.as_ref() == Some(user) || self.held_before.contains(user)
    }
}

/// The rows of `matters.json`, by matter.
pub type Rows = BTreeMap<MatterId, MatterRow>;

/// What `matters.json` holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Contents {
    /// One row per matter.
    pub(super) rows: Rows,
    /// The pool users owed a profile reset (`MatterSids::release` marks one, the elevated
    /// set-up clears it): none goes to another matter until then (`Why::NeedsReset`).
    pub(super) needs_reset: BTreeSet<Sid>,
}

/// `matters.json` as it is written.
#[derive(Serialize)]
struct MattersOut<'a> {
    version: u32,
    matters: &'a Rows,
    /// Always written, empty or not, and required on read: a file without it is refused.
    needs_reset: &'a BTreeSet<Sid>,
}

/// A version 3 file as it is read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MattersV3In {
    /// Read by [`Version`]; listed so `deny_unknown_fields` admits it.
    #[serde(rename = "version")]
    _version: de::IgnoredAny,
    matters: UniqueMatters<MatterRow>,
    /// Required, failing closed: a version 3 file without it is refused, never read as none.
    needs_reset: Vec<Sid>,
}

/// The version alone, read first to choose the shape of the rest.
#[derive(Deserialize)]
struct Version {
    version: u32,
}

/// `matters.json` as it is read, with `V` the value of each matter.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, bound = "V: Deserialize<'de>")]
struct MattersIn<V> {
    /// Read by [`Version`]; listed so `deny_unknown_fields` admits it.
    #[serde(rename = "version")]
    _version: de::IgnoredAny,
    matters: UniqueMatters<V>,
}

/// The matters map, refused when it names one matter twice (JSON allows a repeated key, and a
/// plain map would keep the last silently).
struct UniqueMatters<V>(BTreeMap<MatterId, V>);

impl<'de, V: Deserialize<'de>> Deserialize<'de> for UniqueMatters<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<V>(PhantomData<V>);

        impl<'de, V: Deserialize<'de>> Visitor<'de> for Entries<V> {
            type Value = UniqueMatters<V>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map of matter ids to their records")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut matters = BTreeMap::new();
                while let Some((matter, value)) = map.next_entry::<MatterId, V>()? {
                    if matters.insert(matter, value).is_some() {
                        return Err(de::Error::custom(format!(
                            "matter {matter} is listed twice"
                        )));
                    }
                }
                Ok(UniqueMatters(matters))
            }
        }

        deserializer.deserialize_map(Entries(PhantomData))
    }
}

/// Reads and checks the file `path`, as [`super::MatterSids::load_with`] describes. No file is
/// no rows.
pub(super) fn read(path: &Path, reserved: &Reserved) -> Result<Contents, SetupError> {
    let failed = |why: String| SetupError::Failed(format!("{}: {why}", path.display()));
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Contents::default()),
        Err(error) => return Err(failed(error.to_string())),
    };
    let parse_error = |error: serde_json::Error| failed(error.to_string());
    let version: Version = serde_json::from_slice(&bytes).map_err(parse_error)?;
    let contents = match version.version {
        MATTERS_V1 => {
            let file: MattersIn<Sid> = serde_json::from_slice(&bytes).map_err(parse_error)?;
            let row = |sid| MatterRow {
                sid: Some(sid),
                ..MatterRow::default()
            };
            let rows = file.matters.0.into_iter().map(|(m, sid)| (m, row(sid)));
            Contents {
                rows: rows.collect(),
                needs_reset: BTreeSet::new(),
            }
        }
        MATTERS_V2 => {
            let file: MattersIn<MatterRow> = serde_json::from_slice(&bytes).map_err(parse_error)?;
            let rows = file.matters.0;
            let needs_reset = released_users(&rows);
            Contents { rows, needs_reset }
        }
        MATTERS_VERSION => {
            let file: MattersV3In = serde_json::from_slice(&bytes).map_err(parse_error)?;
            Contents {
                rows: file.matters.0,
                needs_reset: file.needs_reset.into_iter().collect(),
            }
        }
        other => return Err(failed(format!("version {other}, not {MATTERS_VERSION}"))),
    };
    check(&contents, reserved).map_err(failed)?;
    Ok(contents)
}

/// The users `rows` name as released (a released `pool_user`, or one in `held_before`) that no
/// row holds: a version 2 file's, none of them reset since.
fn released_users(rows: &Rows) -> BTreeSet<Sid> {
    let held: BTreeSet<&Sid> = rows.values().filter_map(MatterRow::holds).collect();
    rows.values()
        .flat_map(|row| {
            let released = row.pool_user.iter().filter(|_| row.released_at.is_some());
            released.chain(&row.held_before)
        })
        .filter(|user| !held.contains(user))
        .cloned()
        .collect()
}

/// Refuses rows that can't be right: a row with neither a SID nor a pool user, a release with no
/// pool user, a SID not of the matter form or one `reserved` refuses, one SID held twice or also
/// named as a pool user (or owed a reset), or one pool user held by two matters at once.
fn check(contents: &Contents, reserved: &Reserved) -> Result<(), String> {
    let rows = &contents.rows;
    let mut sids = BTreeSet::new();
    let mut held = BTreeMap::new();
    for (matter, row) in rows {
        if row.sid.is_none() && row.pool_user.is_none() && row.held_before.is_empty() {
            return Err(format!("{matter} has neither a SID nor a pool user"));
        }
        if row.released_at.is_some() && row.pool_user.is_none() {
            return Err(format!("{matter} released a pool user it has none of"));
        }
        if let Some(sid) = &row.sid {
            if !sid.is_matter_form() {
                return Err(format!("{matter}'s SID {sid} is not a matter SID"));
            }
            if !reserved.admits(sid) {
                return Err(format!(
                    "{matter}'s SID {sid} is or shares a domain with an account of this machine"
                ));
            }
            if !sids.insert(sid) {
                return Err(format!("{sid} is held by two matters"));
            }
        }
        if let Some(user) = row.holds()
            && let Some(other) = held.insert(user, matter)
        {
            return Err(format!("pool user {user} is held by {other} and {matter}"));
        }
    }
    let pool_users: BTreeSet<&Sid> = rows
        .values()
        .flat_map(|r| r.pool_user.iter().chain(&r.held_before))
        .chain(&contents.needs_reset)
        .collect();
    if let Some(sid) = sids.iter().find(|sid| pool_users.contains(*sid)) {
        return Err(format!("{sid} is both a matter SID and a pool user"));
    }
    Ok(())
}

/// Writes `contents` to `path` atomically, at [`MATTERS_VERSION`].
pub(super) fn save(path: &Path, contents: &Contents) -> Result<(), SetupError> {
    let failed = |why: String| SetupError::Failed(format!("{}: {why}", path.display()));
    let file = MattersOut {
        version: MATTERS_VERSION,
        matters: &contents.rows,
        needs_reset: &contents.needs_reset,
    };
    let mut bytes = serde_json::to_vec_pretty(&file).map_err(|error| failed(error.to_string()))?;
    bytes.push(b'\n');
    write::write_atomic(path, &bytes).map_err(|error| failed(error.to_string()))
}
