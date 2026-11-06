//! `matters.json`'s format. Version 2 is written: one row per matter, `{ sid?, pool_user?,
//! released_at? }`. Version 1 (matter id to SID) is read as rows with only a SID, and is
//! upgraded on the next write. Any other version is refused.

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
pub(super) const MATTERS_VERSION: u32 = 2;
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
}

impl MatterRow {
    /// The pool user the matter holds now: its `pool_user` unless released.
    pub fn holds(&self) -> Option<&Sid> {
        self.pool_user
            .as_ref()
            .filter(|_| self.released_at.is_none())
    }
}

/// The rows of `matters.json`, by matter.
pub type Rows = BTreeMap<MatterId, MatterRow>;

/// `matters.json` as it is written.
#[derive(Serialize)]
struct MattersOut<'a> {
    version: u32,
    matters: &'a Rows,
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
pub(super) fn read(path: &Path, reserved: &Reserved) -> Result<Rows, SetupError> {
    let failed = |why: String| SetupError::Failed(format!("{}: {why}", path.display()));
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Rows::new()),
        Err(error) => return Err(failed(error.to_string())),
    };
    let parse_error = |error: serde_json::Error| failed(error.to_string());
    let version: Version = serde_json::from_slice(&bytes).map_err(parse_error)?;
    let rows = match version.version {
        MATTERS_V1 => {
            let file: MattersIn<Sid> = serde_json::from_slice(&bytes).map_err(parse_error)?;
            let row = |sid| MatterRow {
                sid: Some(sid),
                ..MatterRow::default()
            };
            file.matters
                .0
                .into_iter()
                .map(|(m, sid)| (m, row(sid)))
                .collect()
        }
        MATTERS_VERSION => {
            let file: MattersIn<MatterRow> = serde_json::from_slice(&bytes).map_err(parse_error)?;
            file.matters.0
        }
        other => return Err(failed(format!("version {other}, not {MATTERS_VERSION}"))),
    };
    check(&rows, reserved).map_err(failed)?;
    Ok(rows)
}

/// Refuses rows that can't be right: a row with neither a SID nor a pool user, a release with no
/// pool user, a SID not of the matter form or one `reserved` refuses, one SID held twice or also
/// named as a pool user, or one pool user held by two matters at once.
fn check(rows: &Rows, reserved: &Reserved) -> Result<(), String> {
    let mut sids = BTreeSet::new();
    let mut held = BTreeMap::new();
    for (matter, row) in rows {
        if row.sid.is_none() && row.pool_user.is_none() {
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
    let pool_users: BTreeSet<&Sid> = rows.values().filter_map(|r| r.pool_user.as_ref()).collect();
    if let Some(sid) = sids.iter().find(|sid| pool_users.contains(*sid)) {
        return Err(format!("{sid} is both a matter SID and a pool user"));
    }
    Ok(())
}

/// Writes `rows` to `path` atomically, at [`MATTERS_VERSION`].
pub(super) fn save(path: &Path, rows: &Rows) -> Result<(), SetupError> {
    let failed = |why: String| SetupError::Failed(format!("{}: {why}", path.display()));
    let file = MattersOut {
        version: MATTERS_VERSION,
        matters: rows,
    };
    let mut bytes = serde_json::to_vec_pretty(&file).map_err(|error| failed(error.to_string()))?;
    bytes.push(b'\n');
    write::write_atomic(path, &bytes).map_err(|error| failed(error.to_string()))
}
