//! The watch key as `--remove` sees it (`teardown.rs` holds the order): every open is
//! `REG_OPTION_OPEN_LINK`, so a registry link at [`WATCH_KEY`] is opened as itself and never
//! followed; [`inspect`] calls one a link when its `SymbolicLinkValue` is `REG_LINK`, and the
//! delete goes through the handle (`NtDeleteKey`), so it takes the key at that name and nothing
//! a link names.
#![expect(unsafe_code, reason = "FFI to the registry and NtDeleteKey")]

use std::io;

use windows::Wdk::System::Registry::NtDeleteKey;
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, HANDLE, RtlNtStatusToDosError,
};
use windows::Win32::Security::Authorization::SE_REGISTRY_KEY;
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
};
use windows::Win32::Storage::FileSystem::READ_CONTROL;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_WOW64_64KEY, REG_LINK, REG_OPTION_OPEN_LINK,
    REG_SAM_FLAGS, REG_VALUE_TYPE, RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetKeySecurity,
};
use windows::core::{HSTRING, w};

use super::protocol::WATCH_KEY;
use super::teardown::{KeyDelete, WatchKey};
use super::windows_pipe::{checked, owner_is_system};
use crate::windows::acl::privilege::Privileges;
use crate::windows::token::os::LocalDescriptor;

/// The DACL a taken key gets: protected; SYSTEM and Administrators full control.
const TAKEN_SDDL: &str = "D:P(A;;KA;;;SY)(A;;KA;;;BA)";
/// `DELETE`, which `NtDeleteKey` needs.
const DELETE: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0001_0000);
/// `WRITE_DAC`, which `RegSetKeySecurity` needs for the DACL (as `record_write.rs` has it).
const WRITE_DAC: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0004_0000);
/// `WRITE_OWNER`, which `RegSetKeySecurity` needs for the owner.
const WRITE_OWNER: REG_SAM_FLAGS = REG_SAM_FLAGS(0x0008_0000);

/// [`WATCH_KEY`] opened as itself, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: a key `open` opened, closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// [`WATCH_KEY`] opened as itself for `access`: `None` when there is none. A refusal is
/// `ERROR_ACCESS_DENIED`.
fn open(access: REG_SAM_FLAGS) -> io::Result<Option<Key>> {
    let mut key = HKEY::default();
    // SAFETY: the key's name is a live HSTRING and `key` a live out-pointer.
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(WATCH_KEY),
            Some(REG_OPTION_OPEN_LINK.0),
            access | KEY_WOW64_64KEY,
            &raw mut key,
        )
    };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    checked(code)?;
    Ok(Some(Key(key)))
}

/// What [`WATCH_KEY`] is: a link before anything else, then by its owner. `Err` when it can't
/// be opened to read its owner, or the owner can't be read.
pub(crate) fn inspect() -> io::Result<WatchKey> {
    let Some(key) = open(REG_SAM_FLAGS(READ_CONTROL.0) | KEY_QUERY_VALUE)? else {
        return Ok(WatchKey::Absent);
    };
    let mut kind = REG_VALUE_TYPE::default();
    // SAFETY: an open key; the value's name is a static wide string; only the type is asked.
    let link = unsafe {
        RegQueryValueExW(
            key.0,
            w!("SymbolicLinkValue"),
            None,
            Some(&raw mut kind),
            None,
            None,
        )
    };
    if checked(link).is_ok() && kind == REG_LINK {
        return Ok(WatchKey::Link);
    }
    Ok(if owner_is_system(HANDLE(key.0.0), SE_REGISTRY_KEY)? {
        WatchKey::System
    } else {
        WatchKey::Other
    })
}

/// Deletes [`WATCH_KEY`] itself through a handle: one that isn't there is
/// [`KeyDelete::Gone`], a refused open (`ERROR_ACCESS_DENIED`) [`KeyDelete::Denied`].
pub(crate) fn delete() -> io::Result<KeyDelete> {
    let key = match open(DELETE) {
        Ok(Some(key)) => key,
        Ok(None) => return Ok(KeyDelete::Gone),
        Err(error) if error.raw_os_error() == i32::try_from(ERROR_ACCESS_DENIED.0).ok() => {
            return Ok(KeyDelete::Denied);
        }
        Err(error) => return Err(error),
    };
    // SAFETY: a key open for DELETE; the handle stays open until `key` drops.
    let status = unsafe { NtDeleteKey(HANDLE(key.0.0)) };
    if status.is_ok() {
        return Ok(KeyDelete::Gone);
    }
    // SAFETY: RtlNtStatusToDosError only maps a number.
    let code = unsafe { RtlNtStatusToDosError(status) };
    Err(io::Error::from_raw_os_error(
        i32::try_from(code).unwrap_or(i32::MAX),
    ))
}

/// Takes [`WATCH_KEY`] for Administrators, elevated, so [`delete`] may delete it: with
/// `SeTakeOwnershipPrivilege` enabled (as `--reclaim` does), opened for `WRITE_OWNER` alone
/// and given the owner Administrators; then, as its owner, opened for `WRITE_DAC` and given
/// [`TAKEN_SDDL`]. Called only for a key [`inspect`] found SYSTEM's. One that isn't there is
/// fine.
pub(crate) fn take_for_admins() -> io::Result<()> {
    let _privilege = Privileges::enable(&["SeTakeOwnershipPrivilege"])?;
    let owner = LocalDescriptor::new("O:BA")?;
    if !set_security(WRITE_OWNER, OWNER_SECURITY_INFORMATION, &owner)? {
        return Ok(());
    }
    let dacl = LocalDescriptor::new(TAKEN_SDDL)?;
    let info = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    set_security(WRITE_DAC, info, &dacl).map(|_| ())
}

/// Opens [`WATCH_KEY`] as itself for `access` and writes `info` of `descriptor` to it: `false`
/// when there is no key.
fn set_security(
    access: REG_SAM_FLAGS,
    info: OBJECT_SECURITY_INFORMATION,
    descriptor: &LocalDescriptor,
) -> io::Result<bool> {
    let Some(key) = open(access)? else {
        return Ok(false);
    };
    // SAFETY: the key is open for `access`, which `info` needs; the descriptor is valid while
    // `descriptor` lives.
    let set = unsafe { RegSetKeySecurity(key.0, info, PSECURITY_DESCRIPTOR(descriptor.raw())) };
    checked(set).map(|()| true)
}
