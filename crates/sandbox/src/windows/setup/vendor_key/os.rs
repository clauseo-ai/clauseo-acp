//! The registry under [`super`]'s [`Tree`](super::Tree), in the 64-bit view. A key is opened by
//! name from its parent's handle with `NtOpenKeyEx`, with `OBJ_OPENLINK` and
//! `REG_OPTION_OPEN_LINK`, so a link is opened and never followed. A read asks for backup intent
//! under the privileges [`secure`](super::secure) holds. A take-over opens with full access, and
//! when that's denied, takes ownership on a `WRITE_OWNER` handle first. The security is written
//! with `SetSecurityInfo`, which applies inheritance to an unprotected DACL (`RegSetKeySecurity`
//! doesn't). Keys are deleted only through the handle they were taken over on.
#![expect(
    unsafe_code,
    reason = "FFI to the NT and Win32 registry and security calls"
)]

use std::ptr::null_mut;

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::System::Registry::{NtDeleteKey, NtOpenKeyEx};
use windows::Win32::Foundation::{
    ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree, NTSTATUS, OBJ_CASE_INSENSITIVE,
    OBJ_OPENLINK, STATUS_ACCESS_DENIED, STATUS_OBJECT_NAME_NOT_FOUND, UNICODE_STRING, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1, SE_REGISTRY_KEY,
    SetSecurityInfo,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_READ, KEY_WOW64_64KEY, REG_LINK,
    REG_OPTION_BACKUP_RESTORE, REG_OPTION_NON_VOLATILE, REG_OPTION_OPEN_LINK, REG_VALUE_TYPE,
    RegCloseKey, RegCreateKeyExW, RegEnumKeyExW, RegGetKeySecurity, RegQueryValueExW,
};
use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR};

use super::{Tree, VENDOR_SDDL};
use crate::windows::setup::lock::Descriptor;

/// `READ_CONTROL`, which the security reads need.
const READ_CONTROL: u32 = 0x0002_0000;

/// `WRITE_OWNER`, which a take-over needs before it can change the owner.
const WRITE_OWNER: u32 = 0x0008_0000;

/// Full access, in the 64-bit view.
const FULL: u32 = KEY_ALL_ACCESS.0 | KEY_WOW64_64KEY.0;

/// Read access, in the 64-bit view.
const READ: u32 = KEY_READ.0 | KEY_WOW64_64KEY.0;

/// A key opened without following a link.
const NO_FOLLOW: u32 = REG_OPTION_OPEN_LINK.0;

/// A read: without following a link, with backup intent.
const READ_OPTIONS: u32 = REG_OPTION_OPEN_LINK.0 | REG_OPTION_BACKUP_RESTORE.0;

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

/// The registry under the key `path` (`SOFTWARE\Clauseo` for set-up), through [`Key`] handles.
pub(super) struct Registry {
    /// The root's path under `HKLM`.
    path: String,
}

impl Registry {
    /// The registry rooted at the key `path` under `HKLM`.
    pub(super) fn at(path: &str) -> Registry {
        Registry {
            path: path.to_owned(),
        }
    }

    /// The root's NT path, which `NtOpenKeyEx` takes without a root handle.
    fn nt_path(&self) -> String {
        format!("\\Registry\\Machine\\{}", self.path)
    }
}

impl Tree for Registry {
    type Key = Key;

    fn open_root(&mut self) -> Result<Option<Key>, String> {
        open(None, &self.nt_path(), READ, READ_OPTIONS)
            .map_err(|status| nt_error("NtOpenKeyEx", status))
    }

    fn create_root(&mut self) -> Result<(), String> {
        create(None, &self.path, VENDOR_SDDL).map(|_key| ())
    }

    fn take_over_root(&mut self, sddl: &str) -> Result<Key, String> {
        take_over(None, &self.nt_path(), sddl)?.ok_or_else(|| "the vendor key is gone".to_owned())
    }

    fn open_child(&mut self, parent: &Key, name: &str) -> Result<Option<Key>, String> {
        open(Some(parent), name, READ, READ_OPTIONS)
            .map_err(|status| nt_error("NtOpenKeyEx", status))
    }

    fn take_over_child(
        &mut self,
        parent: &Key,
        name: &str,
        sddl: &str,
    ) -> Result<Option<Key>, String> {
        take_over(Some(parent), name, sddl)
    }

    fn create_child(&mut self, parent: &Key, name: &str, sddl: &str) -> Result<Key, String> {
        create(Some(parent), name, sddl)
    }

    fn is_link(&mut self, key: &Key) -> Result<bool, String> {
        let name = HSTRING::from(LINK_VALUE);
        let mut kind = REG_VALUE_TYPE::default();
        // SAFETY: the key is open; the name is NUL-terminated; `kind` is a live out-pointer and
        // no data is asked for.
        let got = unsafe { RegQueryValueExW(key.0, &name, None, Some(&raw mut kind), None, None) };
        if got == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
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
        set_security(key, sddl)
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

/// `name` opened without following a link, with `access` and `options`: an absolute NT path
/// when `parent` is `None`, else a name under `parent`. `None` when it is missing.
fn open(
    parent: Option<&Key>,
    name: &str,
    access: u32,
    options: u32,
) -> Result<Option<Key>, NTSTATUS> {
    let units: Vec<u16> = name.encode_utf16().collect();
    // Key names are at most 255 characters, so the length always fits.
    let bytes = u16::try_from(units.len() * 2).unwrap_or(u16::MAX);
    let text = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(units.as_ptr().cast_mut()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
        RootDirectory: parent.map_or(HANDLE::default(), |key| HANDLE(key.0.0)),
        ObjectName: &raw const text,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_OPENLINK,
        ..OBJECT_ATTRIBUTES::default()
    };
    let mut handle = HANDLE::default();
    // SAFETY: `text` points into `units`, which lives for the call and is never written;
    // `attributes` and `handle` are live; the parent's handle, when there is one, is open. The
    // handle made is owned by `Key` from here.
    let status = unsafe { NtOpenKeyEx(&raw mut handle, access, &raw const attributes, options) };
    if status == STATUS_OBJECT_NAME_NOT_FOUND {
        return Ok(None);
    }
    if status.is_err() {
        return Err(status);
    }
    Ok(Some(Key(HKEY(handle.0))))
}

/// `name` made under `parent` (or at the root's path when `parent` is `None`) with `sddl`.
fn create(parent: Option<&Key>, name: &str, sddl: &str) -> Result<Key, String> {
    let descriptor = Descriptor::from_sddl(sddl).map_err(|error| error.to_string())?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    let root = parent.map_or(HKEY_LOCAL_MACHINE, |key| key.0);
    let subkey = HSTRING::from(name);
    let mut opened = HKEY::default();
    // SAFETY: the subkey is NUL-terminated and lives for the call; `attributes` and the
    // descriptor it points to live for the call too; `opened` is a live out-pointer, owned by
    // `Key` once written.
    let made = unsafe {
        RegCreateKeyExW(
            root,
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
    Ok(Key(opened))
}

/// `name` taken over with `sddl`, opened for writing: `None` when it is gone. The key is opened
/// with full access first; when that's denied, the owner becomes Administrators on a
/// `WRITE_OWNER` handle first ([`owned_then_open`]).
fn take_over(parent: Option<&Key>, name: &str, sddl: &str) -> Result<Option<Key>, String> {
    let opened = match open(parent, name, FULL, NO_FOLLOW) {
        Err(status) if status == STATUS_ACCESS_DENIED => owned_then_open(parent, name)?,
        other => other.map_err(|status| nt_error("NtOpenKeyEx", status))?,
    };
    let Some(key) = opened else {
        return Ok(None);
    };
    set_security(&key, sddl)?;
    Ok(Some(key))
}

/// The take-over of a key the caller may not open for writing: its owner becomes
/// Administrators, on a `WRITE_OWNER` handle that `SeTakeOwnershipPrivilege` gives, and then the
/// key opens for writing as its owner.
fn owned_then_open(parent: Option<&Key>, name: &str) -> Result<Option<Key>, String> {
    let owner = open(
        parent,
        name,
        READ_CONTROL | WRITE_OWNER | KEY_WOW64_64KEY.0,
        NO_FOLLOW,
    )
    .map_err(|status| nt_error("NtOpenKeyEx", status))?;
    let Some(owner) = owner else {
        return Ok(None);
    };
    set_security(&owner, "O:BA")?;
    drop(owner);
    open(parent, name, FULL, NO_FOLLOW).map_err(|status| nt_error("NtOpenKeyEx", status))
}

/// Sets `key`'s owner, and its DACL and its protection when `sddl` has a DACL, with
/// `SetSecurityInfo`, which applies inheritance to an unprotected DACL.
fn set_security(key: &Key, sddl: &str) -> Result<(), String> {
    let descriptor = Descriptor::from_sddl(sddl).map_err(|error| error.to_string())?;
    let mut owner = PSID::default();
    let mut defaulted = BOOL::default();
    // SAFETY: the descriptor is valid for the call; `owner` and `defaulted` are live
    // out-pointers; `owner` points into the descriptor, which lives until this function ends.
    unsafe { GetSecurityDescriptorOwner(descriptor.0, &raw mut owner, &raw mut defaulted) }
        .map_err(|error| error.to_string())?;
    let mut info = OWNER_SECURITY_INFORMATION;
    let mut dacl_ptr: Option<*const ACL> = None;
    if let Some(rest) = sddl.split("D:").nth(1) {
        let mut present = BOOL::default();
        let mut dacl: *mut ACL = null_mut();
        // SAFETY: as for the owner; `dacl` points into the descriptor too.
        unsafe {
            GetSecurityDescriptorDacl(
                descriptor.0,
                &raw mut present,
                &raw mut dacl,
                &raw mut defaulted,
            )
        }
        .map_err(|error| error.to_string())?;
        let control = if rest.starts_with('P') {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        info = info | DACL_SECURITY_INFORMATION | control;
        dacl_ptr = Some(dacl.cast_const());
    }
    // SAFETY: the key is open for WRITE_OWNER (and WRITE_DAC when the DACL is set); the owner and
    // the DACL point into `descriptor`, which lives for the call.
    let set = unsafe {
        SetSecurityInfo(
            HANDLE(key.0.0),
            SE_REGISTRY_KEY,
            info,
            Some(owner),
            None,
            dacl_ptr,
            None,
        )
    };
    check("SetSecurityInfo", set)
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
