//! The intent's values in the record key (`intent.rs`), for `WindowsMachine` and
//! `WindowsRemoval`: read with the `record.rs` readers, written in the key `record_write.rs`
//! makes and secures, `OwnerSid` first, and deleted as its values are.

use windows::Win32::System::Registry::RRF_RT_REG_MULTI_SZ;
use windows::core::{HSTRING, PCWSTR};

use super::MachineError;
use super::intent::{Intent, PENDING_GROUP, PENDING_USERS, names_from_units, names_value};
use super::record_values::{OWNER_SID, RegValue, sz};
use super::record_write::{delete_values, open_secured, read_raw, read_texts, set_value};
use crate::windows::record::{RecordError, read_dword, read_units};
use crate::windows::sids::Sid;

/// The intent under `HKLM\<key>`: `OwnerSid` as read (`None` when it isn't a SID, which
/// reading the record refuses anyway), the pending values, and whether the record is partial.
pub(super) fn read_intent(key: &str) -> Result<Intent, MachineError> {
    let raw = read_raw(key)?;
    let subkey = HSTRING::from(key);
    let at = PCWSTR(subkey.as_ptr());
    let [group, users] = [PENDING_GROUP, PENDING_USERS].map(HSTRING::from);
    let group = read_dword(at, PCWSTR(group.as_ptr())).map_err(reading)?;
    let users = read_units(at, PCWSTR(users.as_ptr()), RRF_RT_REG_MULTI_SZ).map_err(reading)?;
    Ok(Intent {
        owner: raw.owner.as_deref().and_then(|text| Sid::parse(text).ok()),
        group: group.is_some_and(|value| value != 0),
        users: users.as_deref().map(names_from_units).unwrap_or_default(),
        partial: raw.partial(),
    })
}

/// Writes the intent under `HKLM\<key>` (made and secured when missing): `OwnerSid` as
/// `owner` first when the key has none and something is pending, then `PendingGroup` and
/// `PendingUsers`, each deleted when `false` or empty.
pub(super) fn write_intent(
    key: &str,
    owner: &Sid,
    group: bool,
    users: &[String],
) -> Result<(), MachineError> {
    if !group && users.is_empty() {
        return clear_intent(key);
    }
    let owner_missing = read_texts(key, &[OWNER_SID])?.iter().all(Option::is_none);
    let opened = open_secured(key)?;
    if owner_missing {
        set_value(&opened, OWNER_SID, &RegValue::Sz(sz(owner.as_str())))?;
    }
    let mut gone = Vec::new();
    if group {
        set_value(&opened, PENDING_GROUP, &RegValue::Dword(1))?;
    } else {
        gone.push(PENDING_GROUP);
    }
    if users.is_empty() {
        gone.push(PENDING_USERS);
    } else {
        set_value(&opened, PENDING_USERS, &names_value(users))?;
    }
    drop(opened);
    delete_values(key, &gone)
}

/// Deletes the pending values under `HKLM\<key>`, and the key once it holds nothing; a missing
/// key or value is `Ok`.
pub(super) fn clear_intent(key: &str) -> Result<(), MachineError> {
    delete_values(key, &[PENDING_GROUP, PENDING_USERS])
}

/// A failed read of an intent value.
fn reading(error: RecordError) -> MachineError {
    match error {
        RecordError::Denied => MachineError::Os {
            call: "RegGetValueW",
            code: windows::Win32::Foundation::ERROR_ACCESS_DENIED.0,
        },
        other @ (RecordError::WrongType
        | RecordError::NotText
        | RecordError::Other(_)
        | RecordError::Malformed(_)) => MachineError::Other(other.to_string()),
    }
}
