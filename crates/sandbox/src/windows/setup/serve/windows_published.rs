//! The published pipe name on Windows (`published.rs`): the server's write and clear of
//! `HKLM\<record key>\SetUpPipe`, the app's read, and the uninstall's delete. The subkey is made
//! with [`PUBLISHED_SDDL`] and given it again before each write, so one an administrator made
//! some other way gets it too; 64-bit view, as the record's readers use.
#![expect(unsafe_code, reason = "FFI to the Win32 registry calls")]

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    SECURITY_ATTRIBUTES,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_64KEY,
    REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, RegCreateKeyExW, RegDeleteKeyExW, RegSetKeySecurity,
};
use windows::core::{HSTRING, PCWSTR};

use super::{PUBLISHED_SDDL, PUBLISHED_SUBKEY, PUBLISHED_VALUE};
use crate::windows::record::read_string;
use crate::windows::setup::MachineError;
use crate::windows::setup::lock::Descriptor;
use crate::windows::setup::record_values::{RegValue, sz};
use crate::windows::setup::record_write::{Key, RECORD_KEY, check, set_value};

/// `WRITE_DAC` and `WRITE_OWNER`, which `RegSetKeySecurity` needs.
const WRITE_DAC_OWNER: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0004_0000 | 0x0008_0000);

/// The subkey below `record_key`.
fn subkey(record_key: &str) -> String {
    format!("{record_key}\\{PUBLISHED_SUBKEY}")
}

/// Writes `token` to the subkey below `record_key`, made when missing, its owner and DACL set
/// again first.
pub(crate) fn publish(record_key: &str, token: &str) -> Result<(), MachineError> {
    let descriptor = Descriptor::from_sddl(PUBLISHED_SDDL)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    let subkey = HSTRING::from(subkey(record_key));
    let mut opened = HKEY::default();
    // SAFETY: the subkey is a NUL-terminated HSTRING; `attributes` and its descriptor live for
    // the call; `opened` is a live out-pointer. The key is closed by `Key`.
    let made = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | KEY_QUERY_VALUE | WRITE_DAC_OWNER | KEY_WOW64_64KEY,
            Some(&raw const attributes),
            &raw mut opened,
            None,
        )
    };
    check("RegCreateKeyExW", made)?;
    let opened = Key(opened);
    // SAFETY: the key is open with WRITE_DAC and WRITE_OWNER; the descriptor is valid and
    // self-relative, and its owner is LOCAL SYSTEM, this process's user.
    let secured = unsafe {
        RegSetKeySecurity(
            opened.0,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            descriptor.0,
        )
    };
    check("RegSetKeySecurity", secured)?;
    set_value(&opened, PUBLISHED_VALUE, &RegValue::Sz(sz(token)))
}

/// The published value below `record_key`, as anyone reads it: `None` when there is none.
pub(crate) fn read(record_key: &str) -> Result<Option<String>, String> {
    let subkey = HSTRING::from(subkey(record_key));
    let value = HSTRING::from(PUBLISHED_VALUE);
    read_string(PCWSTR(subkey.as_ptr()), PCWSTR(value.as_ptr())).map_err(|error| error.to_string())
}

/// The production record's published value, as the app reads it.
pub(crate) fn read_published() -> Result<Option<String>, String> {
    read(RECORD_KEY)
}

/// Deletes the subkey below `record_key` while it still holds `token`: another run's name is
/// left as it is.
pub(crate) fn unpublish(record_key: &str, token: &str) -> Result<(), MachineError> {
    match read(record_key) {
        Ok(Some(found)) if found == token => delete(record_key),
        Ok(_) => Ok(()),
        Err(why) => Err(MachineError::Other(why)),
    }
}

/// Deletes the subkey below `record_key`, whatever it holds; a missing one is `Ok`. The
/// uninstall's clean-up, so the record's key, then empty, goes too.
pub(crate) fn delete(record_key: &str) -> Result<(), MachineError> {
    let subkey = HSTRING::from(subkey(record_key));
    // SAFETY: the subkey is a NUL-terminated HSTRING that lives for the call.
    let deleted = unsafe { RegDeleteKeyExW(HKEY_LOCAL_MACHINE, &subkey, KEY_WOW64_64KEY.0, None) };
    if deleted == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check("RegDeleteKeyExW", deleted)
}
