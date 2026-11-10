//! `HKLM\SOFTWARE\Clauseo\Sandbox`'s values as the set-up writes them (D-B1): a
//! [`MachineRecord`] to registry values ([`encode`]) and back ([`decode`]), and the key's DACL
//! ([`RECORD_SDDL`]). `record_write.rs` makes the Windows calls; `record.rs` holds the readers
//! #317 and the set-up state use, which read the same values.
//!
//! Pure: built on every OS so its tests run everywhere.

use std::fmt::Write as _;
use std::path::PathBuf;

use super::intent::{PENDING_GROUP, PENDING_USERS};
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
/// `ResetOwed` (`REG_MULTI_SZ`, a set-up value): the pool users a stop found while it couldn't
/// take the set-up lock, which it adds here without the lock (read, merge, write, read back),
/// since only the set-up lock guards [`RESET_SIDS`]. Only the stop task, as SYSTEM, writes it.
/// The next stop that holds the lock folds it into `ResetSids` and deletes it, before it writes
/// [`STOP_SEQUENCE`], so no marker clears with a reset still owed only here.
pub const RESET_OWED: &str = "ResetOwed";
/// `InstallId` (`REG_SZ`, a set-up value): 32 lowercase hex, 128 random bits the set-up writes
/// with the first record of an install ([`new_install_id`]) and never again; `--remove` deletes
/// it with the record. Each unclean-session marker records the id it was written under: one
/// whose id isn't the current one belongs to a removed install, whose `Stop*` and `Sweep*`
/// counters went with it, and is stale (`runner::unclean`). An install from before the id has
/// none, and its markers none either.
pub const INSTALL_ID: &str = "InstallId";
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
/// `SweepCompleted` (`REG_QWORD`, a `FILETIME`): when the last completed sweep's walk began,
/// written only by a `--sweep-tasks` run that ends 0, just before [`SWEEP_SEQUENCE`]. For
/// diagnostics only. Deleted with the set-up's values.
pub const SWEEP_COMPLETED: &str = "SweepCompleted";
/// `SweepSequence` (`REG_QWORD`): the number the last completed sweep reserved from
/// [`SWEEP_NEXT`], written after [`SWEEP_COMPLETED`] under the same set-up lock, only by a run
/// that ends 0 and never lower than it was. Absent counts as 0. Deleted with the set-up's
/// values, before [`SWEEP_NEXT`].
pub const SWEEP_SEQUENCE: &str = "SweepSequence";
/// `SweepNext` (`REG_QWORD`, 1 when absent): the number the next sweep reserves, under the
/// set-up lock, before its walk, as [`STOP_NEXT`] is for the stop. A session's end reads it
/// (without the lock, `pool_steps::sequence` says why) as the marker's `M`; the marker is
/// clean once [`SWEEP_SEQUENCE`] `>= M`.
pub const SWEEP_NEXT: &str = "SweepNext";
/// The values the set-up owns, which a rollback or `--remove` deletes; the ones `--adopt`
/// writes stay until `--remove` takes the admin copy away. Each `…Sequence` comes before its
/// `…Next`, so a marker read during a removal can only wait longer (`pool_steps::sequence`).
pub const SETUP_VALUES: [&str; 16] = [
    OWNER_SID,
    GROUP_SID,
    POOL_SIDS,
    DATA_DIR,
    REMOVING,
    RESET_SIDS,
    RESET_OWED,
    INSTALL_ID,
    STOP_COMPLETED,
    STOP_SEQUENCE,
    STOP_NEXT,
    SWEEP_COMPLETED,
    SWEEP_SEQUENCE,
    SWEEP_NEXT,
    PENDING_GROUP,
    PENDING_USERS,
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

/// An install id ([`INSTALL_ID`]) from 16 random bytes: their lowercase hex.
pub fn new_install_id(random: [u8; 16]) -> String {
    random.iter().fold(String::new(), |mut hex, byte| {
        // Writing to a String can't fail.
        let _written = write!(hex, "{byte:02x}");
        hex
    })
}

/// Whether `text` is an install id: 32 lowercase hex.
pub fn is_install_id(text: &str) -> bool {
    text.len() == 32
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
/// with the `record.rs` readers. [`RawRecord::partial`] says whether they are a partial record.
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

impl RawRecord {
    /// Whether these values are a partial record: some of them are there, but [`decode`] reads
    /// them as no record. A malformed value is no partial record, but [`decode`]'s error.
    pub fn partial(&self) -> bool {
        *self != RawRecord::default() && decode(self.clone()) == Ok(None)
    }
}

/// `record` as the five values of the key, in the order they are written: `GroupSid`,
/// `DataDir`, `Removing`, `PoolSids`, then `OwnerSid` last, so a first write that stops part
/// way never leaves a whole record over half of it ([`decode`] reads it as partial). A record
/// whose pool holds its owner is refused here as well as by the engine (split §9).
pub fn encode(record: &MachineRecord) -> Result<Vec<(&'static str, RegValue)>, RecordValueError> {
    if record.pool.contains(&record.owner) {
        return Err(RecordValueError::OwnerInPool);
    }
    let data_dir = record
        .data_dir
        .to_str()
        .ok_or(RecordValueError::DataDirNotText)?;
    Ok(vec![
        (GROUP_SID, RegValue::Sz(sz(record.group.as_str()))),
        (DATA_DIR, RegValue::Sz(sz(data_dir))),
        (REMOVING, RegValue::Dword(u32::from(record.removing))),
        (POOL_SIDS, RegValue::MultiSz(multi_sz(&record.pool))),
        (OWNER_SID, RegValue::Sz(sz(record.owner.as_str()))),
    ])
}

/// The record `raw` holds. `Ok(None)` when it holds no value at all (no key, or an empty one),
/// and when it lacks `OwnerSid`, `GroupSid` or `DataDir`: a partial set-up (an intent alone,
/// or a first write that stopped part way, `intent.rs`), which set-up redoes and `--remove`
/// cleans, never an error that blocks both. A missing `PoolSids` is an empty pool and a missing
/// `Removing` is not removing, as the `record.rs` readers take them. A value that is there but
/// malformed is the error, whatever else is missing: only an administrator could have written it.
pub fn decode(raw: RawRecord) -> Result<Option<MachineRecord>, RecordValueError> {
    let owner = raw
        .owner
        .map(|text| sid(Some(text), OWNER_SID))
        .transpose()?;
    let group = raw
        .group
        .map(|text| sid(Some(text), GROUP_SID))
        .transpose()?;
    let (Some(owner), Some(group), Some(data_dir)) = (owner, group, raw.data_dir) else {
        return Ok(None);
    };
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
mod tests;
