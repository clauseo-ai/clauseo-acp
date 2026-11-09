//! The machine side of `stranded.rs`: the record read as set-up and `--remove` take it, and
//! the survey of the accounts in `ClauseoSandboxUsers` or with a pool user's name, each with
//! set-up's mark read (`NetLocalGroupGetMembers` level 0, `NetUserGetInfo` level 1, and the
//! sign-in screen's hidden list).
#![expect(unsafe_code, reason = "FFI to the Win32 account calls")]

use windows::Win32::NetworkManagement::NetManagement::{
    LOCALGROUP_MEMBERS_INFO_0, MAX_PREFERRED_LENGTH, NERR_UserNotFound, NetLocalGroupGetMembers,
    NetUserGetInfo, UF_DONT_EXPIRE_PASSWD, UF_PASSWD_CANT_CHANGE, USER_INFO_1, USER_PRIV_USER,
};
use windows::Win32::System::Registry::RRF_RT_REG_MULTI_SZ;
use windows::core::{HSTRING, PCWSTR};

use super::accounts::{self, Kind, NetBuffer, net, sid_text};
use super::machine::MachineError;
use super::record_values::{self, DATA_DIR, GROUP_SID, OWNER_SID, POOL_SIDS, REMOVING, RawRecord};
use super::record_write::reading;
use super::stranded::{PoolAccounts, Seen};
use super::windows_machine::Names;
use crate::windows::pool::{MachineRecord, POOL_CAP, pool_name};
use crate::windows::record::{RecordError, read_dword, read_string, read_units};
use crate::windows::sids::Sid;

/// `record_write::read` as set-up and `--remove` take the record (`stranded.rs`): a `PoolSids`
/// of the wrong type or malformed is an empty pool, never trusted for a SID; `OwnerSid`,
/// `GroupSid`, `DataDir` or `Removing` missing, of the wrong type or malformed is
/// [`MachineError::RecordCorrupt`]. A value that can't be read at all is that error.
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
        removing: read_dword(at, removing).map_err(corrupt)?,
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
    let mut found = PoolAccounts {
        group: group.clone(),
        ..PoolAccounts::default()
    };
    if group.is_some() {
        for sid in member_sids(&names.group)? {
            let Some(real) = accounts::name_of(&sid, Kind::User)? else {
                found.foreign += 1;
                continue;
            };
            let marked = marked(&real)?;
            found.seen.push(Seen {
                name: names.engine(real),
                sid,
                in_group: true,
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
        let marked = marked(&real)?;
        found.seen.push(Seen {
            name,
            sid,
            in_group: false,
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

/// Whether the local account `name` (its real name) carries set-up's mark: a standard user
/// with `UF_DONT_EXPIRE_PASSWD | UF_PASSWD_CANT_CHANGE`, as `accounts::create_user` makes each
/// pool user, hidden from the sign-in screen by its `SpecialAccounts\UserList` value 0, as
/// `accounts::hide` writes it. An account that is gone isn't marked.
pub(super) fn marked(name: &str) -> Result<bool, MachineError> {
    let account = HSTRING::from(name);
    let mut buffer: *mut u8 = std::ptr::null_mut();
    // SAFETY: the name is NUL-terminated; the out-pointer is live. The buffer the call
    // allocates is freed by `NetBuffer`.
    let status = unsafe { NetUserGetInfo(PCWSTR::null(), &account, 1, &raw mut buffer) };
    let freed = NetBuffer(buffer);
    if status == NERR_UserNotFound {
        return Ok(false);
    }
    net("NetUserGetInfo", status)?;
    if freed.0.is_null() {
        return Err(MachineError::Other(
            "NetUserGetInfo gave nothing".to_owned(),
        ));
    }
    // SAFETY: on success the buffer holds one USER_INFO_1, live until `freed` drops.
    let info = unsafe { freed.0.cast::<USER_INFO_1>().read_unaligned() };
    let mark = UF_DONT_EXPIRE_PASSWD.0 | UF_PASSWD_CANT_CHANGE.0;
    let flagged = info.usri1_priv == USER_PRIV_USER && info.usri1_flags.0 & mark == mark;
    Ok(flagged && accounts::hidden(name)?)
}
