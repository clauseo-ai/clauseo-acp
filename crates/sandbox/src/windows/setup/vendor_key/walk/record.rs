//! The record's remake, so a crash can't lose it: the new key is written whole under
//! [`RECORD_TEMP_NAME`] and flushed, the old one deleted, and the new one renamed to
//! [`RECORD_NAME`] through its own handle ([`replace`]). A crash leaves the old key, the new one,
//! or both, and the next walk settles that before anything else ([`recover`]). Pure, against
//! [`Tree`].

use super::super::security::{Security, TRUSTED_OWNERS};
use super::super::{
    RECORD_LOCKED_SDDL, RECORD_NAME, RECORD_TEMP_NAME, Report, Tree, Value, VendorFault, pinned,
    unless_gone,
};
use super::drain;
use crate::windows::setup::record_values::{
    DATA_DIR, GROUP_SID, OWNER_SID, POOL_SIDS, REMOVING, RawRecord, decode,
};

/// `REG_SZ`.
const REG_SZ: u32 = 1;

/// `REG_DWORD`.
const REG_DWORD: u32 = 4;

/// `REG_MULTI_SZ`.
const REG_MULTI_SZ: u32 = 7;

/// Makes the record under `parent` again with `pinned` and `values` (none: empty), at `path`:
/// the copy written and flushed, then `old` (the record, taken over) deleted with what is under
/// it, then the copy renamed into place. A key gone under it is done.
pub(super) fn replace<T: Tree>(
    tree: &mut T,
    parent: &T::Key,
    old: Option<T::Key>,
    values: Option<Vec<Value>>,
    pinned: &str,
    path: String,
    report: &mut Report,
) -> Result<(), VendorFault> {
    let made = tree.create_child(parent, RECORD_TEMP_NAME, pinned);
    let Some(copy) = unless_gone(made, VendorFault::Secure)? else {
        return Ok(());
    };
    if let Some(values) = values
        && unless_gone(tree.write_values(&copy, &values), VendorFault::Secure)?.is_none()
    {
        return Ok(());
    }
    if unless_gone(tree.flush(&copy), VendorFault::Secure)?.is_none() {
        return Ok(());
    }
    if let Some(old) = old {
        drain(tree, old)?;
    }
    if unless_gone(tree.rename(&copy, RECORD_NAME), VendorFault::Secure)?.is_some() {
        report.recreated.push(path);
    }
    Ok(())
}

/// Settles a copy an earlier walk left among the root's subkeys `names`, before the walk visits
/// any: taken out of `names` either way, so it is never kept as a stranger's key. Beside the
/// record, it is deleted and the record made again as always. Alone, it is renamed into place
/// (and added to `names`, so the walk visits it as the record) when [`sound`], else deleted.
pub(super) fn recover<T: Tree>(
    tree: &mut T,
    root: &T::Key,
    root_path: &str,
    names: &mut Vec<String>,
    report: &mut Report,
) -> Result<(), VendorFault> {
    let Some(at) = names
        .iter()
        .position(|name| name.eq_ignore_ascii_case(RECORD_TEMP_NAME))
    else {
        return Ok(());
    };
    let name = names.remove(at);
    let path = format!("{root_path}\\{name}");
    let taken = tree.take_over_child(root, &name, RECORD_LOCKED_SDDL);
    let Some((copy, before)) = unless_gone(taken, VendorFault::Secure)? else {
        return Ok(());
    };
    let beside = names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(RECORD_NAME));
    let wanted = pinned(RECORD_NAME).unwrap_or_default();
    if !beside && sound(tree, &copy, &before, &wanted)? {
        if unless_gone(tree.set_sddl(&copy, &wanted), VendorFault::Secure)?.is_none() {
            return Ok(());
        }
        if unless_gone(tree.rename(&copy, RECORD_NAME), VendorFault::Secure)?.is_some() {
            names.push(RECORD_NAME.to_owned());
            report.recovered.push(path);
        }
        return Ok(());
    }
    drain(tree, copy)?;
    report.dropped_copies.push(path);
    Ok(())
}

/// Whether the copy `copy` is one a walk wrote: its SD as it was (`before`) is exactly `pinned`'s
/// DACL, owned by SYSTEM or Administrators; it holds no link value and no subkey; and its values
/// parse as a record. A copy gone under the check is not.
fn sound<T: Tree>(
    tree: &mut T,
    copy: &T::Key,
    before: &str,
    pinned: &str,
) -> Result<bool, VendorFault> {
    let found = Security::parse(before);
    let wanted = Security::parse(pinned);
    if !TRUSTED_OWNERS.contains(&found.owner.as_str())
        || found.open
        || found.protected != wanted.protected
        || found.aces != wanted.aces
    {
        return Ok(false);
    }
    match unless_gone(tree.has_link_value(copy), VendorFault::CleanUp)? {
        Some(false) => {}
        Some(true) | None => return Ok(false),
    }
    match unless_gone(tree.subkeys(copy), VendorFault::CleanUp)? {
        Some(names) if names.is_empty() => {}
        Some(_) | None => return Ok(false),
    }
    let values = unless_gone(tree.read_values(copy), VendorFault::CleanUp)?;
    Ok(values.is_some_and(|values| parses(&values)))
}

/// Whether `values` read as a record, as the readers take it: each record value of its own type,
/// and [`decode`] taking them (none at all is an empty record).
pub(in crate::windows::setup::vendor_key) fn parses(values: &[Value]) -> bool {
    let named = |name: &str| {
        values
            .iter()
            .find(|value| value.name.eq_ignore_ascii_case(name))
    };
    let units = |value: &Value| -> Vec<u16> {
        let (pairs, _odd) = value.data.as_chunks::<2>();
        pairs.iter().copied().map(u16::from_le_bytes).collect()
    };
    let text = |name: &str| -> Result<Option<String>, ()> {
        let Some(value) = named(name) else {
            return Ok(None);
        };
        if value.kind != REG_SZ {
            return Err(());
        }
        let units = units(value);
        let text = units.split(|unit| *unit == 0).next().unwrap_or_default();
        String::from_utf16(text).map(Some).map_err(|_not_text| ())
    };
    let pool = match named(POOL_SIDS) {
        None => None,
        Some(value) if value.kind == REG_MULTI_SZ => Some(units(value)),
        Some(_) => return false,
    };
    let removing = match named(REMOVING) {
        None => None,
        Some(value) if value.kind == REG_DWORD => {
            match <[u8; 4]>::try_from(value.data.as_slice()) {
                Ok(bytes) => Some(u32::from_le_bytes(bytes)),
                Err(_) => return false,
            }
        }
        Some(_) => return false,
    };
    let (Ok(owner), Ok(group), Ok(data_dir)) = (text(OWNER_SID), text(GROUP_SID), text(DATA_DIR))
    else {
        return false;
    };
    decode(RawRecord {
        owner,
        group,
        pool,
        data_dir,
        removing,
    })
    .is_ok()
}
