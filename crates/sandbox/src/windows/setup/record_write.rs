//! Writes, reads and deletes the machine record (`HKLM\SOFTWARE\Clauseo\Sandbox`, D-B1) for
//! `WindowsMachine`. The key is made with `RegCreateKeyExW` in the 64-bit view
//! (`KEY_WOW64_64KEY`), the key the `record.rs` readers' `RRF_SUBKEY_WOW6464KEY` finds, and every
//! write sets its protected DACL ([`RECORD_SDDL`]) again, so a key made some other way gets it
//! too. The values are [`record_values::encode`]'s; reads go through the `record.rs` readers.
#![expect(unsafe_code, reason = "FFI to the Win32 registry calls")]

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::Security::{DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY,
    REG_CREATE_KEY_DISPOSITION, REG_DWORD, REG_MULTI_SZ, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS,
    REG_SZ, RRF_RT_REG_MULTI_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteKeyExW, RegSetKeySecurity,
    RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

use super::MachineError;
use super::lock::Descriptor;
use super::record_values::{
    self, DATA_DIR, GROUP_SID, OWNER_SID, POOL_SIDS, RECORD_SDDL, REMOVING, RawRecord, RegValue,
};
use crate::windows::pool::MachineRecord;
use crate::windows::record::{RecordError, read_dword, read_string, read_units};

/// The record's key under `HKLM`, as `record.rs` names it.
pub(super) const RECORD_KEY: &str = "SOFTWARE\\Clauseo\\Sandbox";

/// `WRITE_DAC`, which `RegSetKeySecurity` needs for the DACL.
const WRITE_DAC: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0004_0000);

/// The record under `HKLM\<key>`; `None` when the key is missing or holds none of its values.
pub(super) fn read(key: &str) -> Result<Option<MachineRecord>, MachineError> {
    let subkey = HSTRING::from(key);
    let at = PCWSTR(subkey.as_ptr());
    let names = [OWNER_SID, GROUP_SID, POOL_SIDS, DATA_DIR, REMOVING].map(HSTRING::from);
    let [owner, group, pool, data_dir, removing] =
        names.each_ref().map(|name| PCWSTR(name.as_ptr()));
    let raw = RawRecord {
        owner: read_string(at, owner).map_err(reading)?,
        group: read_string(at, group).map_err(reading)?,
        pool: read_units(at, pool, RRF_RT_REG_MULTI_SZ).map_err(reading)?,
        data_dir: read_string(at, data_dir).map_err(reading)?,
        removing: read_dword(at, removing).map_err(reading)?,
    };
    record_values::decode(raw).map_err(|error| MachineError::Other(error.to_string()))
}

/// Writes `record` as `HKLM\<key>`'s values, making the key when it is missing, and sets the
/// key's DACL to [`RECORD_SDDL`]. A record whose pool holds its owner is refused before any
/// write.
pub(super) fn write(key: &str, record: &MachineRecord) -> Result<(), MachineError> {
    let values =
        record_values::encode(record).map_err(|error| MachineError::Other(error.to_string()))?;
    let descriptor = Descriptor::from_sddl(RECORD_SDDL)?;
    let attributes = windows::Win32::Security::SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<windows::Win32::Security::SECURITY_ATTRIBUTES>())
            .unwrap_or(u32::MAX),
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    let subkey = HSTRING::from(key);
    let mut opened = HKEY::default();
    let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
    // SAFETY: the subkey is a NUL-terminated HSTRING; `attributes` and its descriptor live for
    // the call; `opened` and `disposition` are live out-pointers. The key is closed by `Key`.
    let made = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_QUERY_VALUE | WRITE_DAC | KEY_WOW64_64KEY,
            Some(&raw const attributes),
            &raw mut opened,
            Some(&raw mut disposition),
        )
    };
    check("RegCreateKeyExW", made)?;
    let opened = Key(opened);
    // SAFETY: the key is open with WRITE_DAC; the descriptor is valid and self-relative.
    let secured = unsafe {
        RegSetKeySecurity(
            opened.0,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        )
    };
    check("RegSetKeySecurity", secured)?;
    for (name, value) in &values {
        set_value(&opened, name, value)?;
    }
    Ok(())
}

/// Deletes `HKLM\<key>` (it has no subkeys). A missing key is `Ok`.
pub(super) fn delete(key: &str) -> Result<(), MachineError> {
    let subkey = HSTRING::from(key);
    // SAFETY: the subkey is a NUL-terminated HSTRING that lives for the call.
    let deleted = unsafe { RegDeleteKeyExW(HKEY_LOCAL_MACHINE, &subkey, KEY_WOW64_64KEY.0, None) };
    if deleted == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check("RegDeleteKeyExW", deleted)
}

/// Sets `name` on the open key `opened` to `value`.
pub(super) fn set_value(opened: &Key, name: &str, value: &RegValue) -> Result<(), MachineError> {
    let kind = match value {
        RegValue::Sz(_) => REG_SZ,
        RegValue::MultiSz(_) => REG_MULTI_SZ,
        RegValue::Dword(_) => REG_DWORD,
    };
    let data = record_values::bytes(value);
    let name = HSTRING::from(name);
    // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated and `data` is a
    // live slice whose length the binding passes.
    let set = unsafe { RegSetValueExW(opened.0, &name, None, kind, Some(&data)) };
    check("RegSetValueExW", set)
}

/// A registry call's result as `Ok`, or [`MachineError::Os`] naming `call`.
pub(super) fn check(call: &'static str, code: WIN32_ERROR) -> Result<(), MachineError> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(MachineError::Os { call, code: code.0 })
    }
}

/// A failed read of a record value.
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

/// An open registry key, closed on drop.
pub(super) struct Key(pub(super) HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key came from RegCreateKeyExW or RegOpenKeyExW and is closed once, here.
        // A failed close leaves nothing to do.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// `HKLM\<key>`'s DACL as SDDL, read back for the tests.
#[cfg(test)]
pub(crate) fn key_sddl(key: &str) -> Result<String, MachineError> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::PSECURITY_DESCRIPTOR;
    use windows::Win32::System::Registry::{RegGetKeySecurity, RegOpenKeyExW};
    use windows::core::PWSTR;

    /// `READ_CONTROL`, which `RegGetKeySecurity` needs.
    const READ_CONTROL: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0002_0000);
    let subkey = HSTRING::from(key);
    let mut opened = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `opened` is a live out-pointer, closed by `Key`.
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            READ_CONTROL | KEY_WOW64_64KEY,
            &raw mut opened,
        )
    };
    check("RegOpenKeyExW", open)?;
    let opened = Key(opened);
    let mut size = 0u32;
    // SAFETY: no buffer asks only for the size, written to `size`.
    let _sized =
        unsafe { RegGetKeySecurity(opened.0, DACL_SECURITY_INFORMATION, None, &raw mut size) };
    // u64 words, so the descriptor is aligned.
    let mut buffer = vec![0u64; usize::try_from(size).unwrap_or(0).div_ceil(8).max(1)];
    let descriptor = PSECURITY_DESCRIPTOR(buffer.as_mut_ptr().cast());
    // SAFETY: `buffer` is live, writable and at least `size` bytes long for the call.
    let got = unsafe {
        RegGetKeySecurity(
            opened.0,
            DACL_SECURITY_INFORMATION,
            Some(descriptor),
            &raw mut size,
        )
    };
    check("RegGetKeySecurity", got)?;
    let mut text = PWSTR::null();
    // SAFETY: the descriptor in `buffer` is valid; `text` is a live out-pointer, freed below.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &raw mut text,
            None,
        )
    }
    .map_err(|error| {
        super::lock::os(
            "ConvertSecurityDescriptorToStringSecurityDescriptorW",
            &error,
        )
    })?;
    // SAFETY: the conversion wrote a NUL-terminated string.
    let sddl = unsafe { text.to_string() }.map_err(|error| MachineError::Other(error.to_string()));
    // SAFETY: allocated by the conversion with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    sddl
}
