//! The machine side of `stranded.rs`: the record read as set-up and `--remove` take it, and
//! the survey of the accounts in `ClauseoSandboxUsers` or with a pool user's name, each with
//! set-up's mark read (`NetLocalGroupGetMembers` level 0, `NetUserGetInfo` level 1, and the
//! sign-in screen's hidden list), and the mark's writes (the comment, `MarkerVersion`).
#![expect(unsafe_code, reason = "FFI to the Win32 account calls")]

use std::path::PathBuf;
use windows::Win32::NetworkManagement::NetManagement::{
    LOCALGROUP_MEMBERS_INFO_0, MAX_PREFERRED_LENGTH, NERR_UserNotFound, NetLocalGroupGetMembers,
    NetUserGetInfo, NetUserSetInfo, UF_ACCOUNTDISABLE, UF_DONT_EXPIRE_PASSWD,
    UF_PASSWD_CANT_CHANGE, USER_ACCOUNT_FLAGS, USER_INFO_1, USER_INFO_1007, USER_INFO_1008,
    USER_PRIV_USER,
};
use windows::Win32::System::Registry::RRF_RT_REG_MULTI_SZ;

use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::accounts::{self, Kind, NetBuffer, net, sid_text};
use super::machine::MachineError;
use super::preflight;
use super::record_values::{
    self, DATA_DIR, GROUP_SID, MARKER_VERSION, OWNER_SID, POOL_SIDS, REMOVING, RawRecord,
};
use super::record_write::{self, reading};
use super::stranded::{self, POOL_COMMENT, PartialRecord, PoolAccounts, Seen};
use super::windows_machine::Names;
use crate::windows::pool::{MachineRecord, POOL_CAP, pool_name};
use crate::windows::record::{RecordError, read_dword, read_string, read_units};
use crate::windows::sids::Sid;

/// `record_write::read` as set-up and `--remove` take the record (`stranded.rs`): a `PoolSids`
/// of the wrong type or malformed is an empty pool, never trusted for a SID; `OwnerSid`,
/// `GroupSid` or `DataDir` missing, of the wrong type or malformed is
/// [`MachineError::RecordCorrupt`]; a `Removing` of the wrong type counts as set. A value
/// that can't be read at all is that error.
pub(super) fn read_untrusting_pool(key: &str) -> Result<Option<MachineRecord>, MachineError> {
    let subkey = HSTRING::from(key);
    let at = PCWSTR(subkey.as_ptr());
    let names = [OWNER_SID, GROUP_SID, POOL_SIDS, DATA_DIR, REMOVING].map(HSTRING::from);
    let [owner, group, pool, data_dir, removing] =
        names.each_ref().map(|name| PCWSTR(name.as_ptr()));
    let pool = match read_units(at, pool, RRF_RT_REG_MULTI_SZ) {
        Err(RecordError::WrongType | RecordError::NotText) => {
            tracing::warn!("the record's PoolSids isn't a list; no SID in it is trusted");
            None
        }
        read => read.map_err(reading)?,
    };
    let raw = RawRecord {
        owner: read_string(at, owner).map_err(corrupt)?,
        group: read_string(at, group).map_err(corrupt)?,
        pool,
        data_dir: read_string(at, data_dir).map_err(corrupt)?,
        // A `Removing` of the wrong type counts as set: a removal started (0049).
        removing: match read_dword(at, removing) {
            Err(RecordError::WrongType | RecordError::NotText) => Some(1),
            read => read.map_err(reading)?,
        },
    };
    record_values::decode_untrusting_pool(raw)
        .map_err(|error| MachineError::RecordCorrupt(error.to_string()))
}

/// A value of the wrong type or not text as corrupt; any other failure as `reading` has it.
fn corrupt(error: RecordError) -> MachineError {
    match error {
        RecordError::WrongType | RecordError::NotText | RecordError::Malformed(_) => {
            MachineError::RecordCorrupt(error.to_string())
        }
        other @ (RecordError::Denied | RecordError::Other(_)) => reading(other),
    }
}

/// The local accounts in the group `names.group` or with a pool user's name (in `names`'
/// form), as `stranded.rs` sorts them.
pub(super) fn pool_accounts(names: &Names) -> Result<PoolAccounts, MachineError> {
    let group = accounts::sid_of(&names.group, Kind::Group)?;
    let marker_version = record_write::read_pool_value(&names.record_key, MARKER_VERSION)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "the record's MarkerVersion can't be read");
            None
        })
        .map(|version| u32::try_from(version).unwrap_or(u32::MAX));
    let mut found = PoolAccounts {
        group: group.clone(),
        machine: preflight::machine_domain()?,
        marker_version,
        ..PoolAccounts::default()
    };
    if group.is_some() {
        for sid in member_sids(&names.group)? {
            let Some(real) = accounts::name_of(&sid, Kind::User)? else {
                let named = accounts::lookup_sid(&sid).ok().flatten();
                found.foreign.push(named.map_or_else(
                    || "an account Windows can't name".to_owned(),
                    |(name, domain, _)| format!("{domain}\\{name}"),
                ));
                continue;
            };
            let Marks { commented, marked } = marks(&real)?;
            found.seen.push(Seen {
                name: names.engine(real),
                sid,
                in_group: true,
                commented,
                marked,
            });
        }
    }
    for name in (1..=POOL_CAP).filter_map(pool_name) {
        let real = names.real(&name);
        let Some(sid) = accounts::sid_of(&real, Kind::User)? else {
            continue;
        };
        if found.seen.iter().any(|seen| seen.sid == sid) {
            continue;
        }
        let Marks { commented, marked } = marks(&real)?;
        found.seen.push(Seen {
            name,
            sid,
            in_group: false,
            commented,
            marked,
        });
    }
    Ok(found)
}

/// The SIDs of the local group `group`'s members (`NetLocalGroupGetMembers` level 0).
fn member_sids(group: &str) -> Result<Vec<Sid>, MachineError> {
    let name = HSTRING::from(group);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    let (mut read, mut total) = (0u32, 0u32);
    // SAFETY: the name is NUL-terminated; the out-pointers are live. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe {
        NetLocalGroupGetMembers(
            PCWSTR::null(),
            &name,
            0,
            &raw mut buffer,
            MAX_PREFERRED_LENGTH,
            &raw mut read,
            &raw mut total,
            None,
        )
    };
    let freed = NetBuffer(buffer);
    net("NetLocalGroupGetMembers", status)?;
    let count = usize::try_from(read).map_err(|error| MachineError::Other(error.to_string()))?;
    if freed.0.is_null() {
        return Ok(Vec::new());
    }
    let size = size_of::<LOCALGROUP_MEMBERS_INFO_0>();
    (0..count)
        .map(|index| {
            // SAFETY: on success the call wrote `read` LOCALGROUP_MEMBERS_INFO_0 entries at the
            // buffer, which lives until `freed` drops; entry `index` starts `index` entries in,
            // and its SID lives as long.
            let entry = unsafe {
                freed
                    .0
                    .add(index * size)
                    .cast::<LOCALGROUP_MEMBERS_INFO_0>()
                    .read_unaligned()
            };
            sid_text(entry.lgrmi0_sid)
        })
        .collect()
}

/// What an account carries of set-up's mark.
struct Marks {
    /// Its comment is exactly `POOL_COMMENT`.
    commented: bool,
    /// The older mark: a standard user with `UF_DONT_EXPIRE_PASSWD | UF_PASSWD_CANT_CHANGE`,
    /// as `accounts::create_user` makes each pool user, hidden from the sign-in screen by its
    /// `SpecialAccounts\UserList` value 0, as `accounts::hide` writes it.
    marked: bool,
}

/// What the local account `name` (its real name) carries of set-up's mark
/// (`NetUserGetInfo` level 1). An account that is gone carries none.
fn marks(name: &str) -> Result<Marks, MachineError> {
    let account = HSTRING::from(name);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    // SAFETY: the name is NUL-terminated; the out-pointer is live. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe { NetUserGetInfo(PCWSTR::null(), &account, 1, &raw mut buffer) };
    let freed = NetBuffer(buffer);
    let none = Marks {
        commented: false,
        marked: false,
    };
    if status == NERR_UserNotFound {
        return Ok(none);
    }
    net("NetUserGetInfo", status)?;
    if freed.0.is_null() {
        return Err(MachineError::Other(
            "NetUserGetInfo gave nothing".to_owned(),
        ));
    }
    // SAFETY: on success the buffer holds one USER_INFO_1, live until `freed` drops.
    let info = unsafe { freed.0.cast::<USER_INFO_1>().read_unaligned() };
    // SAFETY: a null comment is none; else a NUL-terminated string inside the live buffer.
    let comment = (!info.usri1_comment.is_null())
        .then(|| unsafe { info.usri1_comment.to_string() }.ok())
        .flatten();
    let mark = UF_DONT_EXPIRE_PASSWD.0 | UF_PASSWD_CANT_CHANGE.0;
    let flagged = info.usri1_priv == USER_PRIV_USER && info.usri1_flags.0 & mark == mark;
    Ok(Marks {
        commented: comment.as_deref() == Some(POOL_COMMENT),
        marked: flagged && accounts::hidden(name)?,
    })
}

/// Sets the local account `name`'s (its real name) comment to `POOL_COMMENT`
/// (`NetUserSetInfo` level 1007).
pub(super) fn set_comment(name: &str) -> Result<(), MachineError> {
    accounts::checked_name(name)?;
    let mut comment = accounts::wide(POOL_COMMENT);
    let info = USER_INFO_1007 {
        usri1007_comment: PWSTR(comment.as_mut_ptr()),
    };
    let account = HSTRING::from(name);
    // SAFETY: `info` and the comment it points to live for the call; the name is
    // NUL-terminated.
    let status = unsafe {
        NetUserSetInfo(
            PCWSTR::null(),
            &account,
            1007,
            (&raw const info).cast(),
            None,
        )
    };
    net("NetUserSetInfo", status)
}

/// Writes the record's `MarkerVersion` as `stranded::MARKER_VERSION`.
pub(super) fn write_marker_version(key: &str) -> Result<(), MachineError> {
    record_write::write_pool_value(key, MARKER_VERSION, u64::from(stranded::MARKER_VERSION))
}

/// What the corrupt record under `HKLM\<key>` still says: each of `OwnerSid`, `GroupSid` and
/// `DataDir` that reads alone; one that doesn't is `None`.
pub(super) fn read_partial(key: &str) -> PartialRecord {
    let subkey = HSTRING::from(key);
    let at = PCWSTR(subkey.as_ptr());
    let text = |name: &str| {
        let name = HSTRING::from(name);
        read_string(at, PCWSTR(name.as_ptr())).ok().flatten()
    };
    let sid = |name: &str| text(name).and_then(|text| Sid::parse(&text).ok());
    PartialRecord {
        owner: sid(OWNER_SID),
        group: sid(GROUP_SID),
        data_dir: text(DATA_DIR).map(PathBuf::from),
    }
}

/// Disables the local account `name` (its real name): its flags read with `NetUserGetInfo`
/// at level 1, then written back with `UF_ACCOUNTDISABLE` through `NetUserSetInfo` at level
/// 1008. One that is gone is `Ok`.
pub(super) fn disable_account(name: &str) -> Result<(), MachineError> {
    accounts::checked_name(name)?;
    let account = HSTRING::from(name);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    // SAFETY: the name is NUL-terminated; the out-pointer is live. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe { NetUserGetInfo(PCWSTR::null(), &account, 1, &raw mut buffer) };
    let freed = NetBuffer(buffer);
    if status == NERR_UserNotFound {
        return Ok(());
    }
    net("NetUserGetInfo", status)?;
    if freed.0.is_null() {
        return Err(MachineError::Other(
            "NetUserGetInfo gave nothing".to_owned(),
        ));
    }
    // SAFETY: on success the buffer holds one USER_INFO_1, live until `freed` drops.
    let flags = unsafe { freed.0.cast::<USER_INFO_1>().read_unaligned() }.usri1_flags;
    let info = USER_INFO_1008 {
        usri1008_flags: USER_ACCOUNT_FLAGS(flags.0 | UF_ACCOUNTDISABLE.0),
    };
    // SAFETY: `info` lives for the call; the name is NUL-terminated.
    let status = unsafe {
        NetUserSetInfo(
            PCWSTR::null(),
            &account,
            1008,
            (&raw const info).cast(),
            None,
        )
    };
    net("NetUserSetInfo", status)
}
