//! `HKLM\SOFTWARE\Clauseo\Sandbox`'s values as the set-up writes them (D-B1): a
//! [`MachineRecord`] to registry values ([`encode`]) and back ([`decode`]), and the key's DACL
//! ([`RECORD_SDDL`]). `record_write.rs` makes the Windows calls; `record.rs` holds the readers
//! #317 and the set-up state use, which read the same values.
//!
//! Pure: built on every OS so its tests run everywhere.

use std::path::PathBuf;

use crate::windows::pool::{MachineRecord, pool_sids_from_multi_sz};
use crate::windows::sids::Sid;

/// The record key's security descriptor (D-B1): a protected DACL, full control for SYSTEM and
/// Administrators, read for Users (#317 reads the key under a pool token, whose restricting set
/// holds `Users`). Each entry is inherited by subkeys (`CI`).
pub const RECORD_SDDL: &str = "D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)";

/// `OwnerSid` (`REG_SZ`).
pub const OWNER_SID: &str = "OwnerSid";
/// `GroupSid` (`REG_SZ`).
pub const GROUP_SID: &str = "GroupSid";
/// `PoolSids` (`REG_MULTI_SZ`).
pub const POOL_SIDS: &str = "PoolSids";
/// `DataDir` (`REG_SZ`).
pub const DATA_DIR: &str = "DataDir";
/// `Removing` (`REG_DWORD`).
pub const REMOVING: &str = "Removing";

/// One registry value's data, as `RegSetValueExW` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    /// `REG_SZ`: UTF-16 units ending in one NUL.
    Sz(Vec<u16>),
    /// `REG_MULTI_SZ`: each string's units and a NUL, then one more NUL.
    MultiSz(Vec<u16>),
    /// `REG_DWORD`.
    Dword(u32),
}

/// Why a record isn't written or isn't read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordValueError {
    /// `PoolSids` would name the owner: the owner's app would then refuse to start (#317).
    #[error("the record would list its owner as a pool user")]
    OwnerInPool,
    /// `DataDir` is not Unicode text, so it can't be a `REG_SZ` the readers give back as written.
    #[error("the data folder's path is not Unicode")]
    DataDirNotText,
    /// The key has some values but not this one, which every record has.
    #[error("the record has no {0}")]
    Missing(&'static str),
    /// This value doesn't read as its kind (a SID, a list of SIDs).
    #[error("the record's {0} is malformed")]
    Malformed(&'static str),
}

/// The record's values as read, each `None` when missing: what `record_write::read` gathers
/// with the `record.rs` readers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawRecord {
    /// `OwnerSid`'s text.
    pub owner: Option<String>,
    /// `GroupSid`'s text.
    pub group: Option<String>,
    /// `PoolSids`' UTF-16 units, NULs and all.
    pub pool: Option<Vec<u16>>,
    /// `DataDir`'s text.
    pub data_dir: Option<String>,
    /// `Removing`.
    pub removing: Option<u32>,
}

/// `record` as the five values of the key, in the order they are written: `OwnerSid`,
/// `GroupSid`, `DataDir`, `Removing`, then `PoolSids`. A record whose pool holds its owner is
/// refused here as well as by the engine (split §9).
pub fn encode(record: &MachineRecord) -> Result<Vec<(&'static str, RegValue)>, RecordValueError> {
    if record.pool.contains(&record.owner) {
        return Err(RecordValueError::OwnerInPool);
    }
    let data_dir = record
        .data_dir
        .to_str()
        .ok_or(RecordValueError::DataDirNotText)?;
    Ok(vec![
        (OWNER_SID, RegValue::Sz(sz(record.owner.as_str()))),
        (GROUP_SID, RegValue::Sz(sz(record.group.as_str()))),
        (DATA_DIR, RegValue::Sz(sz(data_dir))),
        (REMOVING, RegValue::Dword(u32::from(record.removing))),
        (POOL_SIDS, RegValue::MultiSz(multi_sz(&record.pool))),
    ])
}

/// The record `raw` holds. `Ok(None)` when it holds no value at all (no key, or an empty one).
/// Otherwise `OwnerSid`, `GroupSid` and `DataDir` must be there; a missing `PoolSids` is an
/// empty pool and a missing `Removing` is not removing, as the `record.rs` readers take them.
pub fn decode(raw: RawRecord) -> Result<Option<MachineRecord>, RecordValueError> {
    if raw == RawRecord::default() {
        return Ok(None);
    }
    let owner = sid(raw.owner, OWNER_SID)?;
    let group = sid(raw.group, GROUP_SID)?;
    let data_dir = raw.data_dir.ok_or(RecordValueError::Missing(DATA_DIR))?;
    let pool = match raw.pool {
        Some(units) => pool_sids_from_multi_sz(&units)
            .map_err(|_malformed| RecordValueError::Malformed(POOL_SIDS))?,
        None => Vec::new(),
    };
    Ok(Some(MachineRecord {
        owner,
        group,
        pool,
        data_dir: PathBuf::from(data_dir),
        removing: raw.removing.is_some_and(|value| value != 0),
    }))
}

/// The SID `text` spells, for the value `name`.
fn sid(text: Option<String>, name: &'static str) -> Result<Sid, RecordValueError> {
    let text = text.ok_or(RecordValueError::Missing(name))?;
    Sid::parse(&text).map_err(|_not_a_sid| RecordValueError::Malformed(name))
}

/// `text` as a `REG_SZ`'s data: its UTF-16 units and a NUL.
pub fn sz(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// `sids` as a `REG_MULTI_SZ`'s data: each SID's units and a NUL, then one more NUL. An empty
/// list is the one NUL.
fn multi_sz(sids: &[Sid]) -> Vec<u16> {
    let mut units: Vec<u16> = sids
        .iter()
        .flat_map(|sid| sid.as_str().encode_utf16().chain([0]))
        .collect();
    units.push(0);
    units
}

/// `value`'s data as the bytes `RegSetValueExW` takes: the units in little-endian order (the
/// byte order of every Windows target), or the DWORD's four bytes.
pub fn bytes(value: &RegValue) -> Vec<u8> {
    match value {
        RegValue::Sz(units) | RegValue::MultiSz(units) => {
            units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
        }
        RegValue::Dword(number) => number.to_le_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sid(text: &str) -> Sid {
        Sid::parse(text).unwrap()
    }

    fn record() -> MachineRecord {
        MachineRecord {
            owner: sid("S-1-5-21-7-8-9-1001"),
            group: sid("S-1-5-21-7-8-9-1009"),
            pool: vec![sid("S-1-5-21-7-8-9-1010"), sid("S-1-5-21-7-8-9-1011")],
            data_dir: PathBuf::from(r"C:\Users\Rohan\AppData\Local\Clauseo"),
            removing: false,
        }
    }

    /// The values `encode` gives, read back the way `record_write::read` gathers them.
    fn raw_of(values: &[(&'static str, RegValue)]) -> RawRecord {
        let text = |units: &[u16]| {
            String::from_utf16(units.split(|unit| *unit == 0).next().unwrap()).unwrap()
        };
        let mut raw = RawRecord::default();
        for (name, value) in values {
            match (*name, value) {
                (OWNER_SID, RegValue::Sz(units)) => raw.owner = Some(text(units)),
                (GROUP_SID, RegValue::Sz(units)) => raw.group = Some(text(units)),
                (DATA_DIR, RegValue::Sz(units)) => raw.data_dir = Some(text(units)),
                (POOL_SIDS, RegValue::MultiSz(units)) => raw.pool = Some(units.clone()),
                (REMOVING, RegValue::Dword(number)) => raw.removing = Some(*number),
                other => panic!("unexpected value {other:?}"),
            }
        }
        raw
    }

    #[test]
    fn the_values_are_the_five_the_readers_read_in_order() {
        let values = encode(&record()).unwrap();
        let names: Vec<&str> = values.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            ["OwnerSid", "GroupSid", "DataDir", "Removing", "PoolSids"]
        );
        assert_eq!(values[0].1, RegValue::Sz(sz("S-1-5-21-7-8-9-1001")));
        assert!(matches!(&values[0].1, RegValue::Sz(units) if units.ends_with(&[0])));
        assert_eq!(values[3].1, RegValue::Dword(0));
        let RegValue::MultiSz(pool) = &values[4].1 else {
            panic!("{:?}", values[4]);
        };
        let expected: Vec<u16> = "S-1-5-21-7-8-9-1010\0S-1-5-21-7-8-9-1011\0\0"
            .encode_utf16()
            .collect();
        assert_eq!(pool, &expected);
    }

    #[test]
    fn a_record_round_trips_through_its_values() {
        let mut removing = record();
        removing.removing = true;
        let mut empty = record();
        empty.pool.clear();
        for each in [record(), removing, empty] {
            let raw = raw_of(&encode(&each).unwrap());
            assert_eq!(decode(raw), Ok(Some(each)));
        }
    }

    #[test]
    fn an_empty_pool_is_one_nul_and_reads_back_empty() {
        let mut empty = record();
        empty.pool.clear();
        let values = encode(&empty).unwrap();
        assert_eq!(values[4].1, RegValue::MultiSz(vec![0]));
        assert_eq!(pool_sids_from_multi_sz(&[0]), Ok(vec![]));
    }

    #[test]
    fn the_owner_is_never_written_as_a_pool_user() {
        let mut bad = record();
        bad.pool.push(bad.owner.clone());
        assert_eq!(encode(&bad), Err(RecordValueError::OwnerInPool));
    }

    #[test]
    fn bytes_are_little_endian_units_or_the_dword() {
        assert_eq!(bytes(&RegValue::Sz(sz("Ab"))), [b'A', 0, b'b', 0, 0, 0]);
        assert_eq!(
            bytes(&RegValue::MultiSz(vec![0x263A, 0])),
            [0x3A, 0x26, 0, 0]
        );
        assert_eq!(bytes(&RegValue::Dword(1)), [1, 0, 0, 0]);
    }

    #[test]
    fn no_value_is_no_record_and_a_partial_one_is_refused() {
        assert_eq!(decode(RawRecord::default()), Ok(None));
        let whole = raw_of(&encode(&record()).unwrap());
        let mut no_owner = whole.clone();
        no_owner.owner = None;
        assert_eq!(decode(no_owner), Err(RecordValueError::Missing("OwnerSid")));
        let mut no_group = whole.clone();
        no_group.group = None;
        assert_eq!(decode(no_group), Err(RecordValueError::Missing("GroupSid")));
        let mut no_dir = whole.clone();
        no_dir.data_dir = None;
        assert_eq!(decode(no_dir), Err(RecordValueError::Missing("DataDir")));
        // Missing PoolSids and Removing read as the readers read them.
        let mut bare = whole.clone();
        bare.pool = None;
        bare.removing = None;
        let read = decode(bare).unwrap().unwrap();
        assert_eq!((read.pool.len(), read.removing), (0, false));
        let mut odd = whole.clone();
        odd.removing = Some(7);
        assert!(decode(odd).unwrap().unwrap().removing);
        let mut alias = whole.clone();
        alias.owner = Some("BA".to_owned());
        assert_eq!(decode(alias), Err(RecordValueError::Malformed("OwnerSid")));
        let mut repeated = whole;
        repeated.pool = Some(
            "S-1-5-18\0S-1-5-18\0\0"
                .encode_utf16()
                .collect::<Vec<u16>>(),
        );
        assert_eq!(
            decode(repeated),
            Err(RecordValueError::Malformed("PoolSids"))
        );
    }

    #[test]
    fn the_sddl_is_the_one_the_readers_document() {
        assert_eq!(RECORD_SDDL, "D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)");
    }
}
