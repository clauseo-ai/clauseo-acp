//! The record's key deleted once it is empty, then the vendor key above it,
//! `HKLM\SOFTWARE\Clauseo` ([`VENDOR_KEY`]), once that is empty too, so a removal that took the
//! record leaves no empty key behind. Empty means no value and no subkey (`RegQueryInfoKeyW`):
//! a per-machine install's `SOFTWARE\Clauseo\Clauseo` keeps it. A subkey made between the
//! count and the delete fails the delete (`RegDeleteKeyExW` refuses a key with subkeys); a
//! value made then goes with the key. The firm's policy key, `SOFTWARE\Policies\Clauseo`, is
//! never touched. Pure: tested on every OS; `record_write.rs` is the registry (64-bit view).

use super::MachineError;

/// The key under `HKLM` the record sits in.
pub(super) const VENDOR_KEY: &str = "SOFTWARE\\Clauseo";

/// What a key under `HKLM` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Counts {
    pub(super) subkeys: u32,
    pub(super) values: u32,
}

/// The keys under `HKLM`, in the 64-bit view.
pub(super) trait Keys {
    /// What `HKLM\<key>` holds; `None` when it is missing.
    fn counts(&mut self, key: &str) -> Result<Option<Counts>, MachineError>;
    /// Deletes `HKLM\<key>`, refused when it has a subkey; missing is `Ok`.
    fn delete(&mut self, key: &str) -> Result<(), MachineError>;
}

/// Deletes `key` when it holds nothing; whether it is gone.
fn delete_if_empty(keys: &mut dyn Keys, key: &str) -> Result<bool, MachineError> {
    match keys.counts(key)? {
        None => Ok(true),
        Some(Counts {
            subkeys: 0,
            values: 0,
        }) => keys.delete(key).map(|()| true),
        Some(_) => Ok(false),
    }
}

/// Deletes the record's key `record_key` when it is empty, then, once it is gone, the vendor key
/// it sits directly in when that is empty too. The vendor key's failure is logged, not
/// returned: the record is gone by then, and a key left empty is untidy, not unsafe.
pub(super) fn delete_emptied(keys: &mut dyn Keys, record_key: &str) -> Result<(), MachineError> {
    if !delete_if_empty(keys, record_key)? {
        return Ok(());
    }
    let in_vendor = record_key
        .rsplit_once('\\')
        .is_some_and(|(parent, _)| parent.eq_ignore_ascii_case(VENDOR_KEY));
    if in_vendor && let Err(error) = delete_if_empty(keys, VENDOR_KEY) {
        tracing::warn!(%error, "the empty HKLM\\SOFTWARE\\Clauseo key stays");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
