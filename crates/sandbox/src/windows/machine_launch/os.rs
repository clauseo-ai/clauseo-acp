//! The guard's registry, in the 64-bit view: `HKLM\SOFTWARE\Microsoft\Ole`'s
//! `MachineLaunchRestriction`, `MachineAccessRestriction` and `MachineLaunchRestrictionOld`,
//! COM's effective access descriptor (`super::effective`), Group Policy's
//! `HKLM\SOFTWARE\Policies\Microsoft\Windows NT\DCOM` value of the same name (its SDDL
//! converted by Windows) and whether its `MachineAccessRestriction` is set, the backup in the
//! record's subkey `MachineLaunch` (protected DACL: SYSTEM and Administrators only, so the pool,
//! which reads the record, can't read or change it), and the state in the record itself, which
//! this never makes: a missing record fails the state's write.
//!
//! The backup's values: `Written` (`REG_BINARY`), the bytes last written, set first;
//! `PutBack` (`REG_DWORD`, 1 once a run has put the deny back since the first write);
//! `AccessAdded` (`REG_BINARY`, only when Clauseo wrote `MachineAccessRestriction`: those
//! bytes); `History` (`REG_BINARY`, the last values Clauseo wrote, each as a `u32` length and its
//! bytes); then `Original` (`REG_BINARY`), the value before Clauseo's first change, empty when
//! there was none. A backup counts only once `Original` is there, so a write cut short between
//! the two leaves none, and the value, not yet changed, is taken as the original again.
#![expect(
    unsafe_code,
    reason = "FFI to the registry and Windows' SDDL conversion"
)]

use std::ptr::null_mut;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, HLOCAL, LocalFree,
    WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetSecurityDescriptorLength, IsValidSecurityDescriptor,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SECURITY_ATTRIBUTES,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_CREATE_SUB_KEY, KEY_SET_VALUE, KEY_WOW64_64KEY, REG_BINARY,
    REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RRF_NOEXPAND,
    RRF_RT_ANY, RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegCreateKeyExW, RegDeleteKeyExW,
    RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetKeySecurity, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

use super::STATE_VALUE;
use super::hold::{pack, unpack};
use super::seam::{Backup, LaunchStore, Raw, StoreError};

/// The key COM reads the value from.
const OLE_KEY: &str = "SOFTWARE\\Microsoft\\Ole";
/// The key Group Policy writes its value to.
const POLICY_KEY: &str = "SOFTWARE\\Policies\\Microsoft\\Windows NT\\DCOM";
/// The value's name, in both keys.
const VALUE: &str = "MachineLaunchRestriction";
/// The machine access value DCOM wants beside the launch value.
const ACCESS: &str = "MachineAccessRestriction";
/// Where DCOM moves a launch value it won't keep.
const OLD: &str = "MachineLaunchRestrictionOld";
/// The backup's subkey, under the record.
pub const BACKUP_SUBKEY: &str = "MachineLaunch";
/// The backup's key: owned by Administrators, a protected DACL of SYSTEM and Administrators.
const BACKUP_SDDL: &str = "O:BAD:P(A;;KA;;;SY)(A;;KA;;;BA)";
/// The backup's value holding the original.
const ORIGINAL: &str = "Original";
/// The backup's value holding what was written last.
const WRITTEN: &str = "Written";
/// The backup's value set to 1 once a run has put the deny back (`Backup::put_back`).
const PUT_BACK: &str = "PutBack";
/// The backup's value holding the access value Clauseo wrote (`Backup::access`).
const ACCESS_ADDED: &str = "AccessAdded";
/// The backup's value holding every value Clauseo wrote (`Backup::history`).
const HISTORY: &str = "History";
/// `WRITE_DAC`.
const WRITE_DAC: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0004_0000);
/// `WRITE_OWNER`.
const WRITE_OWNER: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0008_0000);

/// The real registry, with the record at `HKLM\<record_key>`.
pub struct WindowsLaunchStore {
    /// The record's key under `HKLM`.
    record_key: String,
}

impl WindowsLaunchStore {
    /// The registry, with the record at `HKLM\<record_key>`.
    pub fn new(record_key: &str) -> WindowsLaunchStore {
        WindowsLaunchStore {
            record_key: record_key.to_owned(),
        }
    }

    /// The backup's key under `HKLM`.
    fn backup_key(&self) -> String {
        format!("{}\\{BACKUP_SUBKEY}", self.record_key)
    }
}

impl LaunchStore for WindowsLaunchStore {
    fn policy(&mut self) -> Result<Option<Result<Vec<u8>, String>>, StoreError> {
        Ok(match get(POLICY_KEY, VALUE)? {
            None => None,
            Some((REG_SZ, data)) => Some(sddl_bytes(&text(&data))),
            Some(_) => Some(Err("the policy value isn't REG_SZ".to_owned())),
        })
    }

    fn read(&mut self) -> Result<Raw, StoreError> {
        ole(VALUE)
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        valid_descriptor(bytes)?;
        let key = open(OLE_KEY, KEY_SET_VALUE)?
            .ok_or_else(|| "HKLM\\SOFTWARE\\Microsoft\\Ole is missing".to_owned())?;
        set(&key, VALUE, REG_BINARY, bytes)
    }

    fn delete(&mut self) -> Result<(), StoreError> {
        delete_value(OLE_KEY, VALUE)
    }

    fn backup(&mut self) -> Result<Option<Backup>, StoreError> {
        let key = self.backup_key();
        let Some(original) = binary(get(&key, ORIGINAL)?, ORIGINAL)? else {
            return Ok(None);
        };
        let written = binary(get(&key, WRITTEN)?, WRITTEN)?.unwrap_or_default();
        // Missing reads as no put-back (`PutBack` is written before `Original`, so a backup
        // this code wrote always has it). A value of another type counts as a put-back: the
        // removal then keeps the value and takes off only the deny.
        let put_back = match get(&key, PUT_BACK)? {
            None => false,
            Some((REG_DWORD, data)) => data != [0, 0, 0, 0],
            Some(_) => true,
        };
        let access = binary(get(&key, ACCESS_ADDED)?, ACCESS_ADDED)?;
        let history = binary(get(&key, HISTORY)?, HISTORY)?.map_or_else(Vec::new, |b| unpack(&b));
        Ok(Some(Backup {
            original: (!original.is_empty()).then_some(original),
            written,
            put_back,
            access,
            history,
        }))
    }

    fn write_backup(&mut self, backup: &Backup) -> Result<(), StoreError> {
        let record = open(&self.record_key, KEY_CREATE_SUB_KEY)?
            .ok_or_else(|| "the record's key is gone, so nothing was written".to_owned())?;
        let key = backup_key_in(&record)?;
        set(&key, WRITTEN, REG_BINARY, &backup.written)?;
        let put_back = u32::from(backup.put_back).to_le_bytes();
        set(&key, PUT_BACK, REG_DWORD, &put_back)?;
        set(&key, HISTORY, REG_BINARY, &pack(&backup.history))?;
        match &backup.access {
            Some(access) => set(&key, ACCESS_ADDED, REG_BINARY, access)?,
            None => delete_value(&self.backup_key(), ACCESS_ADDED)?,
        }
        let original = backup.original.as_deref().unwrap_or_default();
        set(&key, ORIGINAL, REG_BINARY, original)
    }

    fn delete_backup(&mut self) -> Result<(), StoreError> {
        let key = HSTRING::from(self.backup_key());
        // SAFETY: the subkey is NUL-terminated and outlives the call.
        let deleted = unsafe { RegDeleteKeyExW(HKEY_LOCAL_MACHINE, &key, KEY_WOW64_64KEY.0, None) };
        if gone(deleted) {
            return Ok(());
        }
        check("RegDeleteKeyExW", deleted)
    }

    fn state(&mut self) -> Result<Option<String>, StoreError> {
        Ok(match get(&self.record_key, STATE_VALUE)? {
            Some((REG_SZ, data)) => Some(text(&data)),
            _ => None,
        })
    }

    fn write_state(&mut self, state: &str) -> Result<(), StoreError> {
        let key = open(&self.record_key, KEY_SET_VALUE)?
            .ok_or_else(|| "the record's key is gone, so nothing was written".to_owned())?;
        let mut data = Vec::new();
        for unit in state.encode_utf16().chain([0]) {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        set(&key, STATE_VALUE, REG_SZ, &data)
    }

    fn delete_state(&mut self) -> Result<(), StoreError> {
        delete_value(&self.record_key, STATE_VALUE)
    }

    fn access(&mut self) -> Result<Raw, StoreError> {
        ole(ACCESS)
    }

    fn access_policy(&mut self) -> Result<bool, StoreError> {
        Ok(get(POLICY_KEY, ACCESS)?.is_some())
    }

    fn effective_access(&mut self) -> Result<Vec<u8>, StoreError> {
        let bytes = super::effective::machine_access()?;
        valid_descriptor(&bytes)?;
        Ok(bytes)
    }

    fn write_access(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        valid_descriptor(bytes)?;
        let key = open(OLE_KEY, KEY_SET_VALUE)?
            .ok_or_else(|| "HKLM\\SOFTWARE\\Microsoft\\Ole is missing".to_owned())?;
        set(&key, ACCESS, REG_BINARY, bytes)
    }

    fn delete_access(&mut self) -> Result<(), StoreError> {
        delete_value(OLE_KEY, ACCESS)
    }

    fn old(&mut self) -> Result<Raw, StoreError> {
        ole(OLD)
    }

    fn delete_old(&mut self) -> Result<(), StoreError> {
        delete_value(OLE_KEY, OLD)
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the guard's bounded wait (1 s in all) for DCOM to act on its write; the tests \
                  inject it through the fake store"
    )]
    fn settle(&mut self, millis: u64) {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }
}

/// `name` of the `Ole` key as read.
fn ole(name: &str) -> Result<Raw, StoreError> {
    Ok(match get(OLE_KEY, name)? {
        None => Raw::Absent,
        Some((REG_BINARY, data)) => Raw::Binary(data),
        Some(_) => Raw::OtherType,
    })
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key came from RegOpenKeyExW or RegCreateKeyExW and is closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// `call`'s result as `Ok`, or an error naming `call` and the code.
fn check(call: &str, code: WIN32_ERROR) -> Result<(), StoreError> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{call} failed ({})", code.0))
    }
}

/// Whether `code` says the key or value isn't there.
fn gone(code: WIN32_ERROR) -> bool {
    code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND
}

/// `HKLM\<key>` opened for `access` in the 64-bit view; `None` when it is missing.
fn open(key: &str, access: REG_SAM_FLAGS) -> Result<Option<Key>, StoreError> {
    let subkey = HSTRING::from(key);
    let mut opened = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `opened` is a live out-pointer, closed by `Key`.
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            access | KEY_WOW64_64KEY,
            &raw mut opened,
        )
    };
    if gone(open) {
        return Ok(None);
    }
    check("RegOpenKeyExW", open)?;
    Ok(Some(Key(opened)))
}

/// The backup's subkey under the open record `record`, made when missing, its owner and
/// protected DACL ([`BACKUP_SDDL`]) set again before any value.
fn backup_key_in(record: &Key) -> Result<Key, StoreError> {
    let descriptor = sddl_bytes(BACKUP_SDDL)?;
    // u64 words, so the descriptor is aligned.
    let mut aligned = vec![0u64; descriptor.len().div_ceil(8)];
    // SAFETY: `aligned` holds at least `descriptor.len()` bytes; the two don't overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(
            descriptor.as_ptr(),
            aligned.as_mut_ptr().cast::<u8>(),
            descriptor.len(),
        );
    }
    let pointer = PSECURITY_DESCRIPTOR(aligned.as_mut_ptr().cast());
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
        lpSecurityDescriptor: pointer.0,
        bInheritHandle: false.into(),
    };
    let subkey = HSTRING::from(BACKUP_SUBKEY);
    let mut opened = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `attributes` and the descriptor in `aligned` live
    // for the call; `opened` is a live out-pointer, closed by `Key`.
    let made = unsafe {
        RegCreateKeyExW(
            record.0,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE | WRITE_DAC | WRITE_OWNER | KEY_WOW64_64KEY,
            Some(&raw const attributes),
            &raw mut opened,
            None,
        )
    };
    check("RegCreateKeyExW", made)?;
    let key = Key(opened);
    // SAFETY: the key is open with WRITE_DAC and WRITE_OWNER; the descriptor is valid and
    // self-relative, and its owner is a group an elevated or SYSTEM token may assign.
    let secured = unsafe {
        RegSetKeySecurity(
            key.0,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            pointer,
        )
    };
    check("RegSetKeySecurity", secured)?;
    Ok(key)
}

/// Sets `name` on `key` to `data` of type `kind`.
fn set(key: &Key, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) -> Result<(), StoreError> {
    let name = HSTRING::from(name);
    // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated and `data` is a
    // live slice whose length the binding passes.
    let set = unsafe { RegSetValueExW(key.0, &name, None, kind, Some(data)) };
    check("RegSetValueExW", set)
}

/// Deletes `name` from `HKLM\<key>`; a missing key or value is `Ok`.
fn delete_value(key: &str, name: &str) -> Result<(), StoreError> {
    let Some(opened) = open(key, KEY_SET_VALUE)? else {
        return Ok(());
    };
    let name = HSTRING::from(name);
    // SAFETY: the key is open with KEY_SET_VALUE; the name is NUL-terminated.
    let deleted = unsafe { RegDeleteValueW(opened.0, &name) };
    if gone(deleted) {
        return Ok(());
    }
    check("RegDeleteValueW", deleted)
}

/// `name` of `HKLM\<key>` (64-bit view, unexpanded): its type and bytes; `None` when the key
/// or the value is missing. A value that grows between the size and the read is read again.
fn get(key: &str, name: &str) -> Result<Option<(REG_VALUE_TYPE, Vec<u8>)>, StoreError> {
    let subkey = HSTRING::from(key);
    let value = HSTRING::from(name);
    let flags = RRF_RT_ANY | RRF_NOEXPAND | RRF_SUBKEY_WOW6464KEY;
    for _ in 0..3 {
        let mut size = 0u32;
        // SAFETY: the names are NUL-terminated; no data pointer asks only for the size.
        let sized = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &subkey,
                &value,
                flags,
                None,
                None,
                Some(&raw mut size),
            )
        };
        if gone(sized) {
            return Ok(None);
        }
        check("RegGetValueW", sized)?;
        let mut data = vec![0u8; usize::try_from(size).unwrap_or(0).max(1)];
        let mut written = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let mut kind = REG_VALUE_TYPE::default();
        // SAFETY: `data` is live, writable and `written` bytes long; `kind` is a live
        // out-pointer.
        let read = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &subkey,
                &value,
                flags,
                Some(&raw mut kind),
                Some(data.as_mut_ptr().cast()),
                Some(&raw mut written),
            )
        };
        if read == ERROR_MORE_DATA {
            continue;
        }
        if gone(read) {
            return Ok(None);
        }
        check("RegGetValueW", read)?;
        data.truncate(usize::try_from(written).unwrap_or(0));
        return Ok(Some((kind, data)));
    }
    Err(format!("{name} kept changing while it was read"))
}

/// A backup value's bytes: `None` when missing, an error when it isn't `REG_BINARY`.
fn binary(
    found: Option<(REG_VALUE_TYPE, Vec<u8>)>,
    name: &str,
) -> Result<Option<Vec<u8>>, StoreError> {
    match found {
        None => Ok(None),
        Some((REG_BINARY, data)) => Ok(Some(data)),
        Some(_) => Err(format!("the backup's {name} isn't REG_BINARY")),
    }
}

/// A `REG_SZ` value's bytes as text, up to its first NUL.
fn text(data: &[u8]) -> String {
    let (pairs, _odd) = data.as_chunks::<2>();
    let units: Vec<u16> = pairs
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// The self-relative descriptor Windows makes of `sddl`.
fn sddl_bytes(sddl: &str) -> Result<Vec<u8>, String> {
    let text = HSTRING::from(sddl);
    let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
    // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` is a live
    // out-pointer, and what it receives is freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(text.as_ptr()),
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .map_err(|error| format!("the SDDL doesn't convert ({})", error.code().0))?;
    // SAFETY: the conversion made a valid self-relative descriptor.
    let len = unsafe { GetSecurityDescriptorLength(descriptor) };
    // SAFETY: the descriptor is `len` bytes long and lives until the free below.
    let bytes = unsafe {
        std::slice::from_raw_parts(descriptor.0.cast::<u8>(), usize::try_from(len).unwrap_or(0))
    }
    .to_vec();
    // SAFETY: allocated with LocalAlloc by the conversion and freed here, once.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    Ok(bytes)
}

/// Refuses `bytes` unless Windows finds them a valid descriptor of exactly their length.
fn valid_descriptor(bytes: &[u8]) -> Result<(), StoreError> {
    let mut aligned = vec![0u64; bytes.len().div_ceil(8).max(1)];
    // SAFETY: `aligned` holds at least `bytes.len()` bytes; the two don't overlap.
    unsafe {
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            aligned.as_mut_ptr().cast::<u8>(),
            bytes.len(),
        );
    }
    let pointer = PSECURITY_DESCRIPTOR(aligned.as_mut_ptr().cast());
    // SAFETY: `pointer` addresses `bytes.len()` live, aligned bytes; the calls only read.
    let valid = unsafe { IsValidSecurityDescriptor(pointer) }.as_bool()
        // SAFETY: as above; a valid descriptor's length is read from its own headers.
        && usize::try_from(unsafe { GetSecurityDescriptorLength(pointer) }) == Ok(bytes.len());
    if valid {
        Ok(())
    } else {
        Err("Windows refused the new value as a security descriptor".to_owned())
    }
}
