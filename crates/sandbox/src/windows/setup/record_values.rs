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
/// `AdminCopySha256` (`REG_SZ`): the admin copy's SHA-256, 64 lowercase hex digits, written by
/// `--adopt` (`windows::admin_copy`), not by the set-up, and outside [`MachineRecord`].
pub const ADMIN_COPY_SHA256: &str = "AdminCopySha256";
/// `UserInstallDir` (`REG_SZ`): the install folder `--adopt` was given, absolute. Untrusted:
/// the lawyer can re-point a folder on its way through a junction at any time.
pub const USER_INSTALL_DIR: &str = "UserInstallDir";
/// `AdminCopyVersion` (`REG_SZ`): the admin copy's version, the app's (`tauri.conf.json`'s
/// `version`, the exe's `FileVersion`). `--adopt` refuses to place an older copy.
pub const ADMIN_COPY_VERSION: &str = "AdminCopyVersion";
/// `AdminCopyOwner` (`REG_SZ`): whose install placed the admin copy, as [`CopyOwner::text`]
/// writes it: [`MACHINE_OWNER`] for a per-machine install, else the lawyer's SID in its string
/// form (`S-1-5-21-...`, as `ConvertSidToStringSidW` gives it). The installer reads it before
/// it overwrites the copy.
pub const ADMIN_COPY_OWNER: &str = "AdminCopyOwner";
/// [`ADMIN_COPY_OWNER`]'s text for a per-machine install.
pub const MACHINE_OWNER: &str = "machine";
/// The values `--adopt` writes and `--remove` deletes with the admin copy.
pub const ADMIN_VALUES: [&str; 4] = [
    ADMIN_COPY_SHA256,
    USER_INSTALL_DIR,
    ADMIN_COPY_VERSION,
    ADMIN_COPY_OWNER,
];

/// What `--adopt` records of the admin copy ([`ADMIN_VALUES`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminValues {
    /// `AdminCopySha256`: 64 lowercase hex digits.
    pub sha256: String,
    /// `UserInstallDir`.
    pub install_dir: PathBuf,
    /// `AdminCopyVersion`.
    pub version: String,
    /// `AdminCopyOwner`.
    pub owner: CopyOwner,
}

/// Whose install placed the admin copy (`AdminCopyOwner`): one owner at a time, as D5 has one
/// pool owner, and a per-machine install counts as an owner of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyOwner {
    /// A per-machine install.
    Machine,
    /// A per-user install by this lawyer.
    User(Sid),
}

impl CopyOwner {
    /// The value's text.
    pub fn text(&self) -> &str {
        match self {
            CopyOwner::Machine => MACHINE_OWNER,
            CopyOwner::User(sid) => sid.as_str(),
        }
    }

    /// The owner `text` names; `None` when it is neither [`MACHINE_OWNER`] nor a SID.
    pub fn parse(text: &str) -> Option<CopyOwner> {
        if text == MACHINE_OWNER {
            return Some(CopyOwner::Machine);
        }
        Sid::parse(text).ok().map(CopyOwner::User)
    }
}
/// `ResetSids` (`REG_MULTI_SZ`, appended to the set-up's values): the pool users the stop task
/// (`setup::stop`, as SYSTEM) found running, each owed a profile reset before it runs anything
/// again. Outside [`MachineRecord`]. The next set-up resets each listed user of the pool, even
/// one a matter holds, once no session is open (`resets.rs`), and takes it off the list.
pub const RESET_SIDS: &str = "ResetSids";
/// `StopCompleted` (`REG_QWORD`, a `FILETIME`: 100 ns since 1601, UTC): when the last completed
/// stop's last clean observation began (the pass that found no pool user's process, or the BITS
/// enumeration, whichever began first), not when it was written; written only by a run that
/// ends 0 (every pool user's process gone, every pool BITS job cancelled, `ResetSids` written),
/// just before [`STOP_SEQUENCE`]. A run that is ended, interrupted or exits non-zero never
/// writes it and leaves an earlier one as it is. For diagnostics only: no clearing trusts it.
/// Outside [`MachineRecord`]; deleted with the set-up's values.
pub const STOP_COMPLETED: &str = "StopCompleted";
/// `StopSequence` (`REG_QWORD`): the number the last completed stop reserved from
/// [`STOP_NEXT`], written after [`STOP_COMPLETED`] under the same set-up lock, only by a run
/// that ends 0 and never lower than it was. Deleted with the set-up's values.
pub const STOP_SEQUENCE: &str = "StopSequence";
/// `StopNext` (`REG_QWORD`, 1 when absent): the number the next stop reserves. Each `--stop`
/// reads it under the set-up lock before its first process snapshot, keeps it as its `n` and
/// writes it one higher, whatever the run then does; a completed run writes `n` as
/// [`STOP_SEQUENCE`]. The task runs one instance at a time, so no two reservations overlap.
///
/// The rule for the follow-up's unclean-session marker: it records `StopNext` as it reads it
/// when it is written (M), and must read it under the set-up lock, so it can't interleave with a
/// reservation's read-then-write and record a stale M. The F4 refusal and the marker clear only
/// when `StopSequence >= M`: a stop reserved its number after the marker existed and then
/// completed, so its observation began after the marker. `--remove` deletes `StopNext`,
/// `StopSequence` and `StopCompleted` with the record and should clear the markers too, so no
/// marker outlives its set-up.
pub const STOP_NEXT: &str = "StopNext";
/// `LogonRightAdded` (`REG_SZ`, a SID): the SID set-up added "Allow log on locally" to, the
/// pool group's, written before the grant (`logon_grant.rs`). `--remove` takes the right off
/// that SID alone, then deletes the value with the set-up's. Outside [`MachineRecord`].
pub const LOGON_RIGHT_ADDED: &str = "LogonRightAdded";
/// The values the set-up owns, which a rollback or `--remove` deletes; the ones `--adopt`
/// writes stay until `--remove` takes the admin copy away.
pub const SETUP_VALUES: [&str; 10] = [
    OWNER_SID,
    GROUP_SID,
    POOL_SIDS,
    DATA_DIR,
    REMOVING,
    RESET_SIDS,
    STOP_COMPLETED,
    STOP_SEQUENCE,
    STOP_NEXT,
    LOGON_RIGHT_ADDED,
];

/// `listed` (`ResetSids` as read) with each of `add` it lacks appended, in order.
pub fn resets_with(listed: &[Sid], add: &[Sid]) -> Vec<Sid> {
    let mut all = listed.to_vec();
    for sid in add {
        if !all.contains(sid) {
            all.push(sid.clone());
        }
    }
    all
}

/// `listed` (`ResetSids` as read) without `done`, the users a set-up has reset.
pub fn resets_without(listed: &[Sid], done: &[Sid]) -> Vec<Sid> {
    listed
        .iter()
        .filter(|sid| !done.contains(sid))
        .cloned()
        .collect()
}

/// `sids` as a `REG_MULTI_SZ` value, as `PoolSids` and `ResetSids` are written.
pub fn sids_value(sids: &[Sid]) -> RegValue {
    RegValue::MultiSz(multi_sz(sids))
}

/// One registry value's data, as `RegSetValueExW` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    /// `REG_SZ`: UTF-16 units ending in one NUL.
    Sz(Vec<u16>),
    /// `REG_MULTI_SZ`: each string's units and a NUL, then one more NUL.
    MultiSz(Vec<u16>),
    /// `REG_DWORD`.
    Dword(u32),
    /// `REG_QWORD`.
    Qword(u64),
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
/// byte order of every Windows target), or the DWORD's four bytes, or the QWORD's eight.
pub fn bytes(value: &RegValue) -> Vec<u8> {
    match value {
        RegValue::Sz(units) | RegValue::MultiSz(units) => {
            units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
        }
        RegValue::Dword(number) => number.to_le_bytes().to_vec(),
        RegValue::Qword(number) => number.to_le_bytes().to_vec(),
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
        assert_eq!(
            bytes(&RegValue::Qword(0x0102_0304_0506_0708)),
            [8, 7, 6, 5, 4, 3, 2, 1]
        );
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

    /// The installer (PR 1) reads these names and forms before it overwrites the admin copy; its
    /// own test pins the same strings against this file.
    #[test]
    fn the_admin_copy_values_keep_their_names_and_forms() {
        assert_eq!(
            ADMIN_VALUES,
            [
                "AdminCopySha256",
                "UserInstallDir",
                "AdminCopyVersion",
                "AdminCopyOwner"
            ]
        );
        assert_eq!(OWNER_SID, "OwnerSid");
        assert_eq!(CopyOwner::Machine.text(), "machine");
        let lawyer = CopyOwner::User(sid("S-1-5-21-1-2-3-1001"));
        assert_eq!(lawyer.text(), "S-1-5-21-1-2-3-1001");
        for owner in [CopyOwner::Machine, lawyer] {
            assert_eq!(CopyOwner::parse(owner.text()), Some(owner));
        }
        for bad in ["Machine", "", "lawyer", "machine "] {
            assert_eq!(CopyOwner::parse(bad), None, "{bad}");
        }
        for name in ADMIN_VALUES {
            assert!(!SETUP_VALUES.contains(&name), "{name}");
        }
        // The stop task's marks go with the set-up's values, so `--remove` deletes them.
        assert_eq!(RESET_SIDS, "ResetSids");
        assert!(SETUP_VALUES.contains(&RESET_SIDS));
        // So do its completion's.
        assert_eq!(
            (STOP_COMPLETED, STOP_SEQUENCE, STOP_NEXT),
            ("StopCompleted", "StopSequence", "StopNext")
        );
        for name in [STOP_COMPLETED, STOP_SEQUENCE, STOP_NEXT] {
            assert!(SETUP_VALUES.contains(&name), "{name}");
        }
    }

    #[test]
    fn reset_marks_are_added_once_and_taken_off_alone() {
        let [a, b, c] = ["1001", "1002", "1003"].map(|rid| sid(&format!("S-1-5-21-1-2-3-{rid}")));
        let listed = resets_with(&[], &[a.clone(), b.clone()]);
        assert_eq!(listed, [a.clone(), b.clone()]);
        let listed = resets_with(&listed, &[b.clone(), c.clone(), c.clone()]);
        assert_eq!(listed, [a.clone(), b.clone(), c.clone()]);
        assert_eq!(
            resets_without(&listed, std::slice::from_ref(&b)),
            [a.clone(), c.clone()]
        );
        assert_eq!(resets_without(&listed, &listed), Vec::<Sid>::new());
        // Written as PoolSids is, and read back by the same reader.
        let RegValue::MultiSz(units) = sids_value(&listed) else {
            panic!("a REG_MULTI_SZ");
        };
        assert_eq!(pool_sids_from_multi_sz(&units), Ok(vec![a, b, c]));
    }

    #[test]
    fn the_sddl_is_the_one_the_readers_document() {
        assert_eq!(RECORD_SDDL, "D:P(A;CI;KA;;;SY)(A;CI;KA;;;BA)(A;CI;KR;;;BU)");
    }
}
