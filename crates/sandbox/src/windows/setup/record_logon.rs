//! The record's logon-right values (`logon_grant.rs`, `rights_source.rs`): `LogonRightAdded`,
//! `LogonPolicy` and `LogonPolicyAt`, written and read through `record_write.rs`'s key calls.

use super::MachineError;
use super::record_values::{self, LOGON_POLICY, LOGON_POLICY_AT, LOGON_RIGHT_ADDED, RegValue};
use super::record_write::{delete_values, open_existing, open_secured, read_texts, set_value};
use super::rights_policy::PolicyState;
use crate::windows::sids::Sid;

/// The record's `LogonRightAdded`: the SID set-up added "Allow log on locally" to; `None` when
/// the key or the value is missing. A value that isn't a SID is an error.
pub(super) fn read_logon_grant(key: &str) -> Result<Option<Sid>, MachineError> {
    let texts = read_texts(key, &[LOGON_RIGHT_ADDED])?;
    let Some(text) = texts.into_iter().flatten().next() else {
        return Ok(None);
    };
    Sid::parse(&text).map(Some).map_err(|_not_a_sid| {
        MachineError::Other("the record's LogonRightAdded isn't a SID".into())
    })
}

/// Writes `LogonRightAdded` as `sid` into `HKLM\<key>`, made and secured as
/// `record_write::write` does (set-up records before it grants, which may be before its first
/// record); `None` deletes it, and the key once it is empty.
pub(super) fn write_logon_grant(key: &str, sid: Option<&Sid>) -> Result<(), MachineError> {
    match sid {
        Some(sid) => set_value(
            &open_secured(key)?,
            LOGON_RIGHT_ADDED,
            &RegValue::Sz(record_values::sz(sid.as_str())),
        ),
        None => delete_values(key, &[LOGON_RIGHT_ADDED]),
    }
}

/// Writes `LogonPolicy` as `state` and `LogonPolicyAt` as `at` into `HKLM\<key>`: made and
/// secured as `record_write::write` does when `make` (set-up), else only into a key that is
/// there, never made (the SYSTEM refresh, which must not bring a removed record back).
pub(super) fn write_logon_policy(
    key: &str,
    state: PolicyState,
    at: u64,
    make: bool,
) -> Result<(), MachineError> {
    let opened = if make {
        open_secured(key)?
    } else {
        open_existing(key)?
    };
    set_value(
        &opened,
        LOGON_POLICY,
        &RegValue::Sz(record_values::sz(state.text())),
    )?;
    set_value(&opened, LOGON_POLICY_AT, &RegValue::Qword(at))
}
