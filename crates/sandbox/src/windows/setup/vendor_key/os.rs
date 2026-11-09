//! The registry under [`super`]'s [`Tree`](super::Tree), in the 64-bit view. A key is opened by
//! name from its parent's handle with `NtOpenKeyEx`, with `OBJ_OPENLINK` and
//! `REG_OPTION_OPEN_LINK`, so a link is opened and never followed; only the root's link check
//! opens it once following, to read its name. A read asks for backup intent under the privileges
//! [`secure`](super::secure) holds. A take-over follows [`take_over_steps`]: the owner and DACL
//! as they were read and then written on a restore-intent handle, then full access. Every write
//! is `RegSetKeySecurity` on the key's own handle, which neither computes nor propagates
//! inheritance. Keys are deleted only through the handle they were taken over on. A key not
//! found or deleted (`STATUS_KEY_DELETED`, `ERROR_KEY_DELETED`) is [`Miss::Gone`].
#![expect(
    unsafe_code,
    reason = "FFI to the NT and Win32 registry and security calls"
)]

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::System::Registry::{KeyNameInformation, NtDeleteKey, NtOpenKeyEx, NtQueryKey};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_KEY_DELETED, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HANDLE, HLOCAL,
    LocalFree, NTSTATUS, OBJ_CASE_INSENSITIVE, OBJ_OPENLINK, OBJECT_ATTRIBUTE_FLAGS,
    STATUS_BUFFER_OVERFLOW, STATUS_BUFFER_TOO_SMALL, STATUS_KEY_DELETED,
    STATUS_OBJECT_NAME_NOT_FOUND, STATUS_OBJECT_PATH_NOT_FOUND, UNICODE_STRING, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
    ConvertStringSidToSidW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
    UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_READ, KEY_WOW64_64KEY,
    REG_CREATE_KEY_DISPOSITION, REG_CREATED_NEW_KEY, REG_LINK, REG_OPTION_BACKUP_RESTORE,
    REG_OPTION_NON_VOLATILE, REG_OPTION_OPEN_LINK, REG_SZ, REG_VALUE_TYPE, RegCloseKey,
    RegCreateKeyExW, RegDeleteValueW, RegEnumKeyExW, RegGetKeySecurity, RegQueryValueExW,
    RegSetKeySecurity, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use super::security::{Security, take_over_steps};
use super::{Miss, Tree, VENDOR_SDDL, Value};
use crate::windows::acl::privilege::Privileges;
use crate::windows::record::read_string;
use crate::windows::setup::admin_member::in_administrators;
use crate::windows::setup::lock::Descriptor;
use crate::windows::setup::preflight::machine_domain;
use crate::windows::sids::Sid;

mod values;

/// Read access, in the 64-bit view.
const READ: u32 = KEY_READ.0 | KEY_WOW64_64KEY.0;

/// A key opened without following a link.
const NO_FOLLOW: u32 = REG_OPTION_OPEN_LINK.0;

/// A read: without following a link, with backup intent.
const READ_OPTIONS: u32 = REG_OPTION_OPEN_LINK.0 | REG_OPTION_BACKUP_RESTORE.0;

/// The installer's uninstall key (`UNINSTKEY` in `installer.nsi`), which only administrators
/// write; its `InstallLocation` is the trusted install folder.
const UNINSTALL_KEY: PCWSTR =
    w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Clauseo");

/// How many times a take-over writes the owner and DACL when the owner it reads back isn't the
/// one it wrote (a planter with an older `WRITE_OWNER` handle setting it back) before it fails.
const OWNER_TRIES: usize = 3;

/// The privilege a take-over's restore-intent step needs, enabled around that step alone.
const RESTORE: &str = "SeRestorePrivilege";

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

    /// Subkey `name` of `parent`, read, not following a link: the elevated tests' check.
    #[cfg(test)]
    pub(super) fn open_child(parent: &Key, name: &str) -> Result<Key, Miss> {
        open(Some(parent), name, READ, READ_OPTIONS, false)
    }

    /// The root's NT path, which `NtOpenKeyEx` takes without a root handle.
    fn nt_path(&self) -> String {
        format!("\\Registry\\Machine\\{}", self.path)
    }
}

impl Tree for Registry {
    type Key = Key;

    fn open_root(&mut self) -> Result<Option<Key>, Miss> {
        match open(None, &self.nt_path(), READ, READ_OPTIONS, false) {
            Ok(key) => Ok(Some(key)),
            Err(Miss::Gone) => Ok(None),
            Err(miss) => Err(miss),
        }
    }

    fn create_root(&mut self) -> Result<(), Miss> {
        // Administrators, which the caller's token holds, own it until the take-over makes
        // SYSTEM the owner under SeRestorePrivilege.
        let sddl = VENDOR_SDDL.replacen("O:SY", "O:BA", 1);
        create(None, &self.path, &sddl, false).map(|_key| ())
    }

    fn take_over_root(&mut self, sddl: &str) -> Result<Key, Miss> {
        take_over(None, &self.nt_path(), sddl).map(|(key, _before)| key)
    }

    fn root_leads_elsewhere(&mut self, key: &Key) -> Result<bool, Miss> {
        let own = key_name(key)?;
        let followed = match open(
            None,
            &self.nt_path(),
            READ,
            REG_OPTION_BACKUP_RESTORE.0,
            true,
        ) {
            Ok(followed) => followed,
            // A link to nothing is still a link.
            Err(Miss::Gone) => return Ok(true),
            Err(miss) => return Err(miss),
        };
        Ok(!key_name(&followed)?.eq_ignore_ascii_case(&own))
    }

    fn take_over_child(
        &mut self,
        parent: &Key,
        name: &str,
        sddl: &str,
    ) -> Result<(Key, String), Miss> {
        take_over(Some(parent), name, sddl)
    }

    fn create_child(&mut self, parent: &Key, name: &str, sddl: &str) -> Result<Key, Miss> {
        create(Some(parent), name, sddl, true)
    }

    fn read_values(&mut self, key: &Key) -> Result<Vec<Value>, Miss> {
        values::read(key)
    }

    fn write_values(&mut self, key: &Key, values: &[Value]) -> Result<(), Miss> {
        values::write(key, values)
    }

    fn flush(&mut self, key: &Key) -> Result<(), Miss> {
        values::flush(key)
    }

    fn rename(&mut self, key: &Key, name: &str) -> Result<(), Miss> {
        values::rename(key, name)
    }

    fn retake(&mut self, key: &Key, sddl: &str) -> Result<(), Miss> {
        let name = key_name(key)?;
        let _restore = Privileges::enable(&[RESTORE])
            .map_err(|error| Miss::Failed(format!("{RESTORE}: {error}")))?;
        write_until_owned(key, &name, sddl)
    }

    fn has_link_value(&mut self, key: &Key) -> Result<bool, Miss> {
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

    fn delete_link_value(&mut self, key: &Key) -> Result<(), Miss> {
        let name = HSTRING::from(LINK_VALUE);
        // SAFETY: the key is open with full access; the name is NUL-terminated.
        let deleted = unsafe { RegDeleteValueW(key.0, &name) };
        check("RegDeleteValueW", deleted)
    }

    fn sddl(&mut self, key: &Key) -> Result<String, Miss> {
        read_sddl(key)
    }

    fn set_sddl(&mut self, key: &Key, sddl: &str) -> Result<(), Miss> {
        write(key, sddl)
    }

    fn subkeys(&mut self, key: &Key) -> Result<Vec<String>, Miss> {
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

    fn delete(&mut self, key: Key) -> Result<(), Miss> {
        // SAFETY: the handle is open with DELETE (`KEY_ALL_ACCESS`), and `NtDeleteKey` takes no
        // other pointer. `key` closes it after this call, once.
        let status = unsafe { NtDeleteKey(HANDLE(key.0.0)) };
        nt_check("NtDeleteKey", status)
    }

    fn reset_install_dir(&mut self, key: &Key) -> Result<Option<String>, Miss> {
        let folder = read_string(UNINSTALL_KEY, w!("InstallLocation"))
            .map_err(|error| Miss::Failed(format!("the uninstall key's folder: {error}")))?
            .map(|text| text.trim().trim_matches('"').to_owned())
            .filter(|folder| !folder.is_empty());
        let Some(folder) = folder else {
            // SAFETY: the key is open with full access; the null name is the default value.
            let deleted = unsafe { RegDeleteValueW(key.0, PCWSTR::null()) };
            if deleted != ERROR_FILE_NOT_FOUND {
                check("RegDeleteValueW", deleted)?;
            }
            return Ok(None);
        };
        let data: Vec<u8> = folder
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        // SAFETY: the key is open with full access; the null name is the default value; `data`
        // is a live slice of NUL-terminated UTF-16.
        let set = unsafe { RegSetValueExW(key.0, PCWSTR::null(), None, REG_SZ, Some(&data)) };
        check("RegSetValueExW", set)?;
        Ok(Some(folder))
    }

    fn local_domain(&mut self) -> Result<[u32; 3], String> {
        machine_domain().map_err(|error| error.to_string())
    }

    fn in_administrators(&mut self, sid: &str) -> Result<bool, String> {
        let sid = Sid::parse(&sid_text(sid)?).map_err(|error| error.to_string())?;
        in_administrators(&sid).map_err(|error| error.to_string())
    }

    fn spell_out(&mut self, alias: &str) -> Result<String, String> {
        sid_text(alias)
    }
}

/// `name` opened with `access` and `options`, following a link only when `follow`: an absolute
/// NT path when `parent` is `None`, else a name under `parent`.
fn open(
    parent: Option<&Key>,
    name: &str,
    access: u32,
    options: u32,
    follow: bool,
) -> Result<Key, Miss> {
    let units: Vec<u16> = name.encode_utf16().collect();
    // Key paths here are far shorter than 32,767 characters, so the length always fits.
    let bytes = u16::try_from(units.len() * 2).unwrap_or(u16::MAX);
    let text = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(units.as_ptr().cast_mut()),
    };
    let link = if follow {
        OBJECT_ATTRIBUTE_FLAGS::default()
    } else {
        OBJ_OPENLINK
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).unwrap_or(0),
        RootDirectory: parent.map_or(HANDLE::default(), |key| HANDLE(key.0.0)),
        ObjectName: &raw const text,
        Attributes: OBJ_CASE_INSENSITIVE | link,
        ..OBJECT_ATTRIBUTES::default()
    };
    let options = if follow {
        options & !REG_OPTION_OPEN_LINK.0
    } else {
        options
    };
    let mut handle = HANDLE::default();
    // SAFETY: `text` points into `units`, which lives for the call and is never written;
    // `attributes` and `handle` are live; the parent's handle, when there is one, is open. The
    // handle made is owned by `Key` from here.
    let status = unsafe { NtOpenKeyEx(&raw mut handle, access, &raw const attributes, options) };
    nt_check("NtOpenKeyEx", status)?;
    Ok(Key(HKEY(handle.0)))
}

/// The key's full NT name (`NtQueryKey`, `KeyNameInformation`), which needs no access right.
fn key_name(key: &Key) -> Result<String, Miss> {
    let mut size = 0u32;
    // SAFETY: no buffer asks only for the size, written to `size`.
    let sized = unsafe { NtQueryKey(HANDLE(key.0.0), KeyNameInformation, None, 0, &raw mut size) };
    if sized != STATUS_BUFFER_TOO_SMALL && sized != STATUS_BUFFER_OVERFLOW {
        nt_check("NtQueryKey", sized)?;
    }
    // u32 words: KEY_NAME_INFORMATION is a u32 length, then that many bytes of UTF-16.
    let mut buffer = vec![0u32; usize::try_from(size).unwrap_or(0).div_ceil(4).max(1)];
    let length = u32::try_from(buffer.len() * 4).unwrap_or(u32::MAX);
    // SAFETY: `buffer` is live, writable and `length` bytes long for the call.
    let status = unsafe {
        NtQueryKey(
            HANDLE(key.0.0),
            KeyNameInformation,
            Some(buffer.as_mut_ptr().cast()),
            length,
            &raw mut size,
        )
    };
    nt_check("NtQueryKey", status)?;
    let bytes = usize::try_from(buffer.first().copied().unwrap_or(0)).unwrap_or(0);
    let units: Vec<u16> = buffer
        .get(1..)
        .unwrap_or_default()
        .iter()
        .flat_map(|word| {
            let [a, b, c, d] = word.to_le_bytes();
            [u16::from_le_bytes([a, b]), u16::from_le_bytes([c, d])]
        })
        .take(bytes / 2)
        .collect();
    Ok(String::from_utf16_lossy(&units))
}

/// A SID's text in the `S-1-...` form, an SDDL alias (`LA`, `DA`) converted.
fn sid_text(text: &str) -> Result<String, String> {
    if text.starts_with("S-1-") {
        return Ok(text.to_owned());
    }
    let wide = HSTRING::from(text);
    let mut sid = PSID::default();
    // SAFETY: `wide` is NUL-terminated and lives for the call; `sid` is a live out-pointer,
    // freed below.
    unsafe { ConvertStringSidToSidW(PCWSTR(wide.as_ptr()), &raw mut sid) }
        .map_err(|error| error.to_string())?;
    let mut out = PWSTR::null();
    // SAFETY: `sid` is valid until freed below; `out` is a live out-pointer, freed below.
    let converted = unsafe { ConvertSidToStringSidW(sid, &raw mut out) };
    // SAFETY: allocated by ConvertStringSidToSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(sid.0))) };
    converted.map_err(|error| error.to_string())?;
    // SAFETY: the conversion wrote a NUL-terminated string.
    let found = unsafe { out.to_string() }.map_err(|error| error.to_string());
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(out.0.cast()))) };
    found
}

/// `name` made under `parent` (or at the root's path when `parent` is `None`) with `sddl`;
/// when `fresh`, a key that was there already is closed unused and fails.
fn create(parent: Option<&Key>, name: &str, sddl: &str, fresh: bool) -> Result<Key, Miss> {
    let descriptor =
        Descriptor::from_sddl(sddl).map_err(|error| Miss::Failed(error.to_string()))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(0),
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    let root = parent.map_or(HKEY_LOCAL_MACHINE, |key| key.0);
    let subkey = HSTRING::from(name);
    let mut opened = HKEY::default();
    let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
    // SAFETY: the subkey is NUL-terminated and lives for the call; `attributes` and the
    // descriptor it points to live for the call too; `opened` and `disposition` are live
    // out-pointers, the key owned by `Key` once written.
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
            Some(&raw mut disposition),
        )
    };
    check("RegCreateKeyExW", made)?;
    let key = Key(opened);
    if fresh && disposition != REG_CREATED_NEW_KEY {
        return Err(Miss::Failed(format!("{name} was there already")));
    }
    Ok(key)
}

/// The key's owner and DACL as SDDL.
fn read_sddl(key: &Key) -> Result<String, Miss> {
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
    converted.map_err(|error| Miss::Failed(error.to_string()))?;
    // SAFETY: the conversion wrote a NUL-terminated string.
    let sddl = unsafe { text.to_string() }.map_err(|error| Miss::Failed(error.to_string()));
    // SAFETY: allocated by the conversion with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    sddl
}

/// A hook the elevated tests set, called with a key's name.
#[cfg(test)]
pub(super) type Hook = Box<dyn FnMut(&str)>;

#[cfg(test)]
thread_local! {
    /// Called with the key's name after each owner and DACL write of a take-over, before the
    /// owner is read back: the elevated tests' planter, setting the owner back.
    pub(super) static AFTER_OWNER_WRITE: std::cell::RefCell<Option<Hook>> =
        std::cell::RefCell::new(None);
    /// Set, a rename fails: the elevated tests' crash after the old record is deleted.
    pub(super) static FAIL_RENAME: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Set, a flush fails: the elevated tests' crash before the old record is deleted.
    pub(super) static FAIL_FLUSH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `name` taken over with `sddl` ([`take_over_steps`]), opened for writing, with its owner and
/// DACL as they were. The restore-intent step runs with `SeRestorePrivilege` enabled around it
/// alone, reads the SD on its handle, and writes until the owner read back is `sddl`'s, at most
/// [`OWNER_TRIES`] times.
fn take_over(parent: Option<&Key>, name: &str, sddl: &str) -> Result<(Key, String), Miss> {
    let mut opened = None;
    let mut before = None;
    for step in take_over_steps(sddl) {
        drop(opened.take());
        let key = if step.restore {
            let _restore = Privileges::enable(&[RESTORE])
                .map_err(|error| Miss::Failed(format!("{RESTORE}: {error}")))?;
            let options = NO_FOLLOW | REG_OPTION_BACKUP_RESTORE.0;
            let key = open(parent, name, step.access, options, false)?;
            before = Some(read_sddl(&key)?);
            if let Some(sddl) = &step.sddl {
                write_until_owned(&key, name, sddl)?;
            }
            key
        } else {
            open(parent, name, step.access, NO_FOLLOW, false)?
        };
        opened = Some(key);
    }
    match (opened, before) {
        (Some(key), Some(before)) => Ok((key, before)),
        _ => Err(Miss::Failed("a take-over has no restore step".to_owned())),
    }
}

/// Writes `sddl`'s owner and DACL to `key` in one call, and reads the owner back; again while
/// it isn't `sddl`'s, up to [`OWNER_TRIES`] writes, then fails closed.
fn write_until_owned(key: &Key, name: &str, sddl: &str) -> Result<(), Miss> {
    let wanted = Security::parse(sddl).owner;
    for _ in 0..OWNER_TRIES {
        write(key, sddl)?;
        #[cfg(test)]
        AFTER_OWNER_WRITE.with_borrow_mut(|hook| {
            if let Some(hook) = hook {
                hook(name);
            }
        });
        if Security::parse(&read_sddl(key)?).owner == wanted {
            return Ok(());
        }
    }
    Err(Miss::Failed(format!(
        "{name}'s owner was set back {OWNER_TRIES} times during its take-over"
    )))
}

/// What `sddl` sets: the owner when it has one, and the DACL with its protection when it has
/// one.
fn parts(sddl: &str) -> OBJECT_SECURITY_INFORMATION {
    let mut info = OBJECT_SECURITY_INFORMATION(0);
    if sddl.contains("O:") {
        info |= OWNER_SECURITY_INFORMATION;
    }
    if let Some(rest) = sddl.split("D:").nth(1) {
        let control = if rest.starts_with('P') {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        info |= DACL_SECURITY_INFORMATION | control;
    }
    info
}

/// Writes `sddl`'s owner and DACL to `key` with `RegSetKeySecurity`: as given, with no
/// inheritance computed and nothing passed down to the subkeys.
fn write(key: &Key, sddl: &str) -> Result<(), Miss> {
    let descriptor =
        Descriptor::from_sddl(sddl).map_err(|error| Miss::Failed(error.to_string()))?;
    // SAFETY: the key is open for what `sddl` sets (WRITE_OWNER, WRITE_DAC); the descriptor is
    // valid and self-relative for the call.
    let set = unsafe { RegSetKeySecurity(key.0, parts(sddl), descriptor.0) };
    check("RegSetKeySecurity", set)
}

/// A registry call's result: `Ok`, [`Miss::Gone`] for a deleted key, or its error naming
/// `call`.
fn check(call: &'static str, code: WIN32_ERROR) -> Result<(), Miss> {
    match code {
        ERROR_SUCCESS => Ok(()),
        ERROR_KEY_DELETED => Err(Miss::Gone),
        _ => Err(Miss::Failed(format!("{call} failed (error {})", code.0))),
    }
}

/// An NT call's result: `Ok`, [`Miss::Gone`] for a key not found or deleted, or its status
/// naming `call`.
fn nt_check(call: &'static str, status: NTSTATUS) -> Result<(), Miss> {
    if status.is_ok() {
        return Ok(());
    }
    if [
        STATUS_OBJECT_NAME_NOT_FOUND,
        STATUS_OBJECT_PATH_NOT_FOUND,
        STATUS_KEY_DELETED,
    ]
    .contains(&status)
    {
        return Err(Miss::Gone);
    }
    Err(Miss::Failed(format!(
        "{call} failed (status {:#010x})",
        status.0.cast_unsigned()
    )))
}
