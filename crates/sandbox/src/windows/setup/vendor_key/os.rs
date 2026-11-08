//! The registry under [`super`]'s [`Tree`](super::Tree), in the 64-bit view. A key is opened by
//! its name from its parent's handle, with `NtOpenKeyEx` and `REG_OPTION_OPEN_LINK`, so a link is
//! opened and never followed; it is secured and walked through those handles, and deleted with
//! `NtDeleteKey` on its handle, so a name swapped under the walk redirects nothing. Every open
//! asks for backup intent, under [`RECLAIM`](crate::windows::acl::privilege::RECLAIM), which
//! lets set-up open a key a weaker run's user owns.
#![expect(unsafe_code, reason = "FFI to the NT and Win32 registry calls")]

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::System::Registry::{NtDeleteKey, NtOpenKeyEx};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree, NTSTATUS,
    OBJ_CASE_INSENSITIVE, STATUS_OBJECT_NAME_NOT_FOUND, UNICODE_STRING, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_WOW64_64KEY, REG_LINK, REG_OPTION_BACKUP_RESTORE,
    REG_OPTION_NON_VOLATILE, REG_OPTION_OPEN_LINK, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW,
    RegEnumKeyExW, RegGetKeySecurity, RegQueryValueExW, RegSetKeySecurity,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::{Tree, VENDOR_SDDL};
use crate::windows::setup::lock::Descriptor;

/// The vendor key by its NT path, opened with no root handle.
const ROOT: &str = "\\Registry\\Machine\\SOFTWARE\\Clauseo";

/// The vendor key by its Win32 path, for `RegCreateKeyExW` in `HKEY_LOCAL_MACHINE`.
const VENDOR: &str = "SOFTWARE\\Clauseo";

/// The value a registry link holds, of type `REG_LINK`.
const LINK_VALUE: &str = "SymbolicLinkValue";

/// An open key, closed on drop.
pub(super) struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key came from `NtOpenKeyEx` or `RegCreateKeyExW` and is closed once, here.
        // A failed close leaves nothing to do.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// The vendor key's registry, through [`Key`] handles.
pub(super) struct Registry;

impl Tree for Registry {
    type Key = Key;

    fn open_root(&mut self) -> Result<Option<Key>, String> {
        open(None, ROOT)
    }

    fn create_root(&mut self) -> Result<(), String> {
        let descriptor = Descriptor::from_sddl(VENDOR_SDDL).map_err(|error| error.to_string())?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: false.into(),
        };
        let subkey = HSTRING::from(VENDOR);
        let mut opened = HKEY::default();
        // SAFETY: the subkey is NUL-terminated and lives for the call; `attributes` and the
        // descriptor it points to live for the call too; `opened` is a live out-pointer, closed
        // by `Key` at once. The owner is SYSTEM, which `acl::privilege` lets the token assign.
        let made = unsafe {
            RegCreateKeyExW(
                HKEY_LOCAL_MACHINE,
                &subkey,
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS | KEY_WOW64_64KEY,
                Some(&raw const attributes),
                &raw mut opened,
                None,
            )
        };
        check("RegCreateKeyExW", made)?;
        drop(Key(opened));
        Ok(())
    }

    fn is_link(&mut self, key: &Key) -> Result<bool, String> {
        let name = HSTRING::from(LINK_VALUE);
        let mut kind = REG_VALUE_TYPE::default();
        // SAFETY: the key is open; the name is NUL-terminated; `kind` is a live out-pointer and
        // no data is asked for.
        let got = unsafe { RegQueryValueExW(key.0, &name, None, Some(&raw mut kind), None, None) };
        if got == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        check("RegQueryValueExW", got)?;
        Ok(kind == REG_LINK)
    }

    fn sddl(&mut self, key: &Key) -> Result<String, String> {
        let wanted = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
        let mut size = 0u32;
        // SAFETY: no buffer asks only for the size, which is written to `size`.
        let _sized = unsafe { RegGetKeySecurity(key.0, wanted, None, &raw mut size) };
        // u64 words, so the descriptor is aligned.
        let mut buffer = vec![0u64; usize::try_from(size).unwrap_or(0).div_ceil(8).max(1)];
        let descriptor = PSECURITY_DESCRIPTOR(buffer.as_mut_ptr().cast());
        // SAFETY: `buffer` is live, writable and at least `size` bytes long for the call.
        let got = unsafe { RegGetKeySecurity(key.0, wanted, Some(descriptor), &raw mut size) };
        check("RegGetKeySecurity", got)?;
        let mut text = PWSTR::null();
        // SAFETY: the descriptor in `buffer` is valid; `text` is a live out-pointer, freed below.
        let converted = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                wanted,
                &raw mut text,
                None,
            )
        };
        converted.map_err(|error| error.to_string())?;
        // SAFETY: the conversion wrote a NUL-terminated string.
        let sddl = unsafe { text.to_string() }.map_err(|error| error.to_string());
        // SAFETY: allocated by the conversion with LocalAlloc, freed once.
        unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
        sddl
    }

    fn set_sddl(&mut self, key: &Key, sddl: &str) -> Result<(), String> {
        let descriptor = Descriptor::from_sddl(sddl).map_err(|error| error.to_string())?;
        let wanted = OWNER_SECURITY_INFORMATION
            | DACL_SECURITY_INFORMATION
            | PROTECTED_DACL_SECURITY_INFORMATION;
        // SAFETY: the key is open with WRITE_DAC and WRITE_OWNER (`KEY_ALL_ACCESS`); the
        // descriptor is valid and self-relative for the call.
        let set = unsafe { RegSetKeySecurity(key.0, wanted, descriptor.0) };
        check("RegSetKeySecurity", set)
    }

    fn subkeys(&mut self, key: &Key) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        for index in 0u32.. {
            let mut name = [0u16; 256];
            let mut len = 256u32;
            // SAFETY: `name` is live and `len` units long for the call, and `len` is a live
            // out-pointer; the class and the last-write time are not asked for.
            let status = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &raw mut len,
                    None,
                    None,
                    None,
                    None,
                )
            };
            if status == ERROR_NO_MORE_ITEMS {
                break;
            }
            check("RegEnumKeyExW", status)?;
            let units = name
                .get(..usize::try_from(len).unwrap_or(0))
                .unwrap_or_default();
            names.push(String::from_utf16_lossy(units));
        }
        Ok(names)
    }

    fn open_child(&mut self, parent: &Key, name: &str) -> Result<Key, String> {
        open(Some(parent), name)?.ok_or_else(|| format!("{name} is gone"))
    }

    fn delete(&mut self, key: Key) -> Result<(), String> {
        // SAFETY: the handle is open with DELETE (`KEY_ALL_ACCESS`), and `NtDeleteKey` takes no
        // other pointer. `key` closes it after this call, once.
        let status = unsafe { NtDeleteKey(HANDLE(key.0.0)) };
        if status.is_err() {
            return Err(nt_error("NtDeleteKey", status));
        }
        Ok(())
    }
}

/// `name` opened without following a link: an absolute NT path when `parent` is `None`, else a
/// name under `parent`. `None` when it is missing.
fn open(parent: Option<&Key>, name: &str) -> Result<Option<Key>, String> {
    let units: Vec<u16> = name.encode_utf16().collect();
    let bytes = u16::try_from(units.len() * 2).map_err(|error| error.to_string())?;
    let text = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(units.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
        RootDirectory: parent.map_or(HANDLE::default(), |key| HANDLE(key.0.0)),
        ObjectName: &raw const text,
        Attributes: OBJ_CASE_INSENSITIVE,
        ..OBJECT_ATTRIBUTES::default()
    };
    let access = KEY_ALL_ACCESS.0 | KEY_WOW64_64KEY.0;
    let options = REG_OPTION_OPEN_LINK.0 | REG_OPTION_BACKUP_RESTORE.0;
    let mut handle = HANDLE::default();
    // SAFETY: `text` points into `units`, which lives for the call and is never written;
    // `attributes` and `handle` are live; the parent's handle, when there is one, is open. The
    // handle made is owned by `Key` from here.
    let status = unsafe { NtOpenKeyEx(&raw mut handle, access, &raw const attributes, options) };
    if status == STATUS_OBJECT_NAME_NOT_FOUND {
        return Ok(None);
    }
    if status.is_err() {
        return Err(nt_error("NtOpenKeyEx", status));
    }
    Ok(Some(Key(HKEY(handle.0))))
}

/// A registry call's result as `Ok`, or its error naming `call`.
fn check(call: &'static str, code: WIN32_ERROR) -> Result<(), String> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("{call} failed (error {})", code.0))
    }
}

/// An NT call's failure, as its status.
fn nt_error(call: &'static str, status: NTSTATUS) -> String {
    format!("{call} failed (status {:#010x})", status.0.cast_unsigned())
}
