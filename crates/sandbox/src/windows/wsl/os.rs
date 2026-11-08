//! The WSL seams on the real registry (`HKLM`, the 64-bit view). The reads need no privilege:
//! `Users` can read every key the launch check reads (the VM's R5c read). The writes open the
//! `AppID`'s key with `REG_OPTION_BACKUP_RESTORE` and `SeBackupPrivilege` and
//! `SeRestorePrivilege` enabled (as wslservice writes its P9NP key), so a key `TrustedInstaller`
//! owns is written without taking its ownership or touching its DACL; the open never makes a
//! key that isn't there. The record's two values go through the set-up's record writer.
//! Errors name the call and its code, never a path or a SID.
#![expect(unsafe_code, reason = "FFI to the Win32 registry calls")]

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_SET_VALUE,
    KEY_WOW64_64KEY, REG_BINARY, REG_CREATE_KEY_DISPOSITION, REG_CREATED_NEW_KEY,
    REG_OPTION_BACKUP_RESTORE, RRF_RT_REG_BINARY, RRF_SUBKEY_WOW6464KEY, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyExW, RegDeleteValueW, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::record::{AppId, WslRecord};
use super::seam::{ClassEntry, Permission, WslError, WslRead, WslWrite};
use crate::windows::acl::privilege::{BACKUP_RESTORE, Privileges};
use crate::windows::record::{read_string, sandbox_group_sid};
use crate::windows::sids::Sid;

/// Where services are installed.
const SERVICES_KEY: &str = r"SYSTEM\CurrentControlSet\Services";
/// Where classes are registered.
const CLSID_KEY: &str = r"SOFTWARE\Classes\CLSID";
/// Where `AppID`s are.
const APPID_KEY: &str = r"SOFTWARE\Classes\AppID";
/// `LxssUserSession`'s packaged-COM index entry.
const PACKAGED_KEY: &str =
    r"SOFTWARE\Classes\PackagedCom\ClassIndex\{A9B7A1B9-0671-405C-95F1-E0612CB4CE7E}";
/// COM's machine-wide settings, which hold `DefaultLaunchPermission`.
const OLE_KEY: &str = r"SOFTWARE\Microsoft\Ole";
/// The value the deny goes in.
const LAUNCH_PERMISSION: &str = "LaunchPermission";
/// The most bytes a launch permission may have.
const VALUE_CAP: u32 = 64 * 1024;

/// The real registry: [`WindowsWsl::reader`] for the launch check, [`WindowsWsl::writer`] for
/// the elevated runs.
#[derive(Debug, Clone)]
pub struct WindowsWsl {
    /// The machine record's key under `HKLM`; `None` for a reader, whose record writes fail.
    record_key: Option<String>,
}

impl WindowsWsl {
    /// The launch check's: reads only.
    pub fn reader() -> WindowsWsl {
        WindowsWsl { record_key: None }
    }

    /// An elevated run's, recording its targets in `HKLM\<record_key>`.
    pub fn writer(record_key: &str) -> WindowsWsl {
        WindowsWsl {
            record_key: Some(record_key.to_owned()),
        }
    }

    fn record_key(&self) -> Result<&str, WslError> {
        self.record_key
            .as_deref()
            .ok_or_else(|| WslError("the launch check writes nothing".to_owned()))
    }
}

/// An open key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful open and is closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// `call` failed with `code`.
fn failed(call: &'static str, code: WIN32_ERROR) -> WslError {
    WslError(format!("{call} failed ({})", code.0))
}

/// Whether a registry call's `code` says the key or value isn't there: `Ok(false)`.
fn found(call: &'static str, code: WIN32_ERROR) -> Result<bool, WslError> {
    match code {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        other => Err(failed(call, other)),
    }
}

/// `HKLM\<path>` opened to read and list it; `None` when it isn't there.
fn open(path: &str) -> Result<Option<Key>, WslError> {
    let subkey = HSTRING::from(path);
    let mut opened = HKEY::default();
    let access = KEY_QUERY_VALUE | KEY_ENUMERATE_SUB_KEYS | KEY_WOW64_64KEY;
    // SAFETY: the subkey is NUL-terminated; `opened` is a live out-pointer, closed by `Key`.
    let code = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, &subkey, None, access, &raw mut opened) };
    Ok(found("RegOpenKeyExW", code)?.then_some(Key(opened)))
}

/// Whether `HKLM\<path>` is there.
fn exists(path: &str) -> Result<bool, WslError> {
    Ok(open(path)?.is_some())
}

/// The `REG_SZ` `value` of `HKLM\<path>`; `None` when it or the key is missing.
fn text(path: &str, value: &str) -> Result<Option<String>, WslError> {
    let (subkey, name) = (HSTRING::from(path), HSTRING::from(value));
    read_string(PCWSTR(subkey.as_ptr()), PCWSTR(name.as_ptr()))
        .map_err(|error| WslError(error.to_string()))
}

/// The `REG_BINARY` `value` of `HKLM\<path>`, at most [`VALUE_CAP`] bytes; `None` when it or
/// the key is missing.
fn binary(path: &str, value: &str) -> Result<Option<Vec<u8>>, WslError> {
    let (subkey, name) = (HSTRING::from(path), HSTRING::from(value));
    let flags = RRF_RT_REG_BINARY | RRF_SUBKEY_WOW6464KEY;
    let mut size = 0u32;
    // SAFETY: the names are NUL-terminated; no data pointer asks only for the size.
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &name,
            flags,
            None,
            None,
            Some(&raw mut size),
        )
    };
    if !found("RegGetValueW", code)? {
        return Ok(None);
    }
    if size > VALUE_CAP {
        return Err(WslError(format!("a value of {size} bytes, over the cap")));
    }
    let mut buffer = vec![0u8; usize::try_from(size).unwrap_or(0)];
    // SAFETY: `buffer` is live, writable and `size` bytes long for the call.
    let code = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &name,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if !found("RegGetValueW", code)? {
        return Ok(None);
    }
    buffer.truncate(usize::try_from(size).unwrap_or(0));
    Ok(Some(buffer))
}

/// The names of `key`'s subkeys.
fn subkeys(key: &Key) -> Result<Vec<String>, WslError> {
    let mut names = Vec::new();
    for index in 0u32.. {
        let mut name = [0u16; 256];
        let mut len = 256u32;
        // SAFETY: `name` is live and `len` units long for the call, and `len` is a live
        // out-pointer; the class and the last-write time are not asked for.
        let code = unsafe {
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
        if code == ERROR_NO_MORE_ITEMS {
            break;
        }
        if code != ERROR_SUCCESS {
            return Err(failed("RegEnumKeyExW", code));
        }
        let units = name
            .get(..usize::try_from(len).unwrap_or(0))
            .unwrap_or_default();
        names.push(String::from_utf16_lossy(units));
    }
    Ok(names)
}

/// `app_id`'s key under `HKLM`.
fn app_key(app_id: &AppId) -> String {
    format!(r"{APPID_KEY}\{app_id}")
}

/// `app_id`'s key opened for a backup-restore write, with the privileges that grant it held
/// for as long as the pair lives. A key the open made (it wasn't there) is deleted again and
/// is the error.
fn open_to_write(app_id: &AppId) -> Result<(Key, Privileges), WslError> {
    let privileges = Privileges::enable(&BACKUP_RESTORE)
        .map_err(|error| WslError(format!("the backup and restore privileges: {error}")))?;
    let path = app_key(app_id);
    let subkey = HSTRING::from(path.as_str());
    let mut opened = HKEY::default();
    let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
    // SAFETY: the subkey is NUL-terminated; `opened` and `disposition` are live out-pointers;
    // no security attributes. With REG_OPTION_BACKUP_RESTORE the access asked is replaced by
    // what the enabled privileges grant. The key is closed by `Key`.
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_BACKUP_RESTORE,
            KEY_SET_VALUE | KEY_QUERY_VALUE | KEY_WOW64_64KEY,
            None,
            &raw mut opened,
            Some(&raw mut disposition),
        )
    };
    if code != ERROR_SUCCESS {
        return Err(failed("RegCreateKeyExW", code));
    }
    let key = Key(opened);
    if disposition == REG_CREATED_NEW_KEY {
        drop(key);
        // SAFETY: the subkey is NUL-terminated; the key was made by this call alone.
        let _deleted =
            unsafe { RegDeleteKeyExW(HKEY_LOCAL_MACHINE, &subkey, KEY_WOW64_64KEY.0, None) };
        return Err(WslError("the AppID's key wasn't there".to_owned()));
    }
    Ok((key, privileges))
}

impl WslRead for WindowsWsl {
    fn service_exists(&self, name: &str) -> Result<bool, WslError> {
        exists(&format!(r"{SERVICES_KEY}\{name}"))
    }

    fn class(&self, clsid: &str) -> Result<ClassEntry, WslError> {
        let path = format!(r"{CLSID_KEY}\{clsid}");
        if !exists(&path)? {
            return Ok(ClassEntry::Missing);
        }
        match text(&path, "AppID")? {
            None => Ok(ClassEntry::NoAppId),
            Some(id) => AppId::parse(&id)
                .map(ClassEntry::AppId)
                .ok_or_else(|| WslError("a WSL class's AppID isn't a GUID".to_owned())),
        }
    }

    fn service_app_ids(&self, services: &[&str]) -> Result<Vec<AppId>, WslError> {
        let Some(key) = open(APPID_KEY)? else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        // Subkeys that aren't GUIDs (an exe's name, mapping it to its `AppID`) hold no service.
        for app_id in subkeys(&key)?.iter().filter_map(|name| AppId::parse(name)) {
            // One key this user can't read, or whose `LocalService` isn't a `REG_SZ`, is
            // logged and passed over: any other program's key could be so, and one such key
            // mustn't refuse every launch. WSL's own `AppID`s are found from [`CLASSES`]
            // too, and those reads stay strict.
            let service = match text(&app_key(&app_id), "LocalService") {
                Ok(service) => service,
                Err(error) => {
                    tracing::warn!(%app_id, %error, "an AppID's LocalService, passed over");
                    continue;
                }
            };
            let wanted = service
                .as_deref()
                .is_some_and(|s| services.iter().any(|w| w.eq_ignore_ascii_case(s.trim())));
            if wanted {
                found.push(app_id);
            }
        }
        Ok(found)
    }

    fn launch_permission(&self, app_id: &AppId) -> Result<Permission, WslError> {
        let path = app_key(app_id);
        if !exists(&path)? {
            return Ok(Permission::NoKey);
        }
        Ok(match binary(&path, LAUNCH_PERMISSION)? {
            Some(bytes) => Permission::Bytes(bytes),
            None => Permission::Absent,
        })
    }

    fn packaged_class(&self) -> Result<bool, WslError> {
        exists(PACKAGED_KEY)
    }

    fn group(&self) -> Option<Sid> {
        let text = sandbox_group_sid().ok().flatten()?;
        Sid::parse(&text).ok()
    }
}

impl WslWrite for WindowsWsl {
    fn default_launch_permission(&self) -> Result<Option<Vec<u8>>, WslError> {
        binary(OLE_KEY, "DefaultLaunchPermission")
    }

    fn write_launch_permission(&mut self, app_id: &AppId, bytes: &[u8]) -> Result<(), WslError> {
        let (key, _privileges) = open_to_write(app_id)?;
        let name = HSTRING::from(LAUNCH_PERMISSION);
        // SAFETY: the key is open for writing; the name is NUL-terminated and `bytes` is a live
        // slice whose length the binding passes.
        let code = unsafe { RegSetValueExW(key.0, &name, None, REG_BINARY, Some(bytes)) };
        if code != ERROR_SUCCESS {
            return Err(failed("RegSetValueExW", code));
        }
        Ok(())
    }

    fn delete_launch_permission(&mut self, app_id: &AppId) -> Result<(), WslError> {
        let (key, _privileges) = open_to_write(app_id)?;
        let name = HSTRING::from(LAUNCH_PERMISSION);
        // SAFETY: the key is open for writing; the name is NUL-terminated.
        let code = unsafe { RegDeleteValueW(key.0, &name) };
        found("RegDeleteValueW", code).map(|_either| ())
    }

    fn recorded(&self) -> Result<WslRecord, WslError> {
        let [app_ids, absent] = crate::windows::setup::read_wsl(self.record_key()?)
            .map_err(|error| WslError(error.to_string()))?;
        WslRecord::decode(app_ids.as_deref(), absent.as_deref())
    }

    fn record(&mut self, record: &WslRecord) -> Result<(), WslError> {
        let (app_ids, absent) = record.encode();
        crate::windows::setup::write_wsl(self.record_key()?, app_ids, absent)
            .map_err(|error| WslError(error.to_string()))
    }

    fn forget(&mut self) -> Result<(), WslError> {
        crate::windows::setup::delete_wsl(self.record_key()?)
            .map_err(|error| WslError(error.to_string()))
    }
}
