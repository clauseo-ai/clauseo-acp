//! The non-pool profiles the stop's WSL step reads ([`VmStopper::non_pool_vm_folder`],
//! [`VmStopper::profile_sids`]): `ProfileList`'s SIDs, and a profile's `Local AppData` redirect
//! from its hive, `HKU\<sid>` when it is loaded, else its `NTUSER.DAT` loaded read-only for the
//! read (`RegLoadAppKeyW`, a private app hive, unloaded when its key is closed). Errors name the
//! call and its code, never a path or a SID.
//!
//! [`VmStopper::non_pool_vm_folder`]: super::VmStopper::non_pool_vm_folder
//! [`VmStopper::profile_sids`]: super::VmStopper::profile_sids
#![expect(
    unsafe_code,
    reason = "FFI to the registry: HKEY_USERS, ProfileList and app hives"
)]

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, KEY_WOW64_64KEY, REG_PROCESS_APPKEY,
    RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegLoadAppKeyW, RegOpenKeyExW,
};
use windows::core::{HSTRING, PWSTR, w};

use super::{Failed, VmId, local_app_data};
use crate::windows::setup::profile::profile_of;
use crate::windows::sids::Sid;

/// `ProfileList`, under `HKLM`.
const PROFILE_LIST: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList";
/// The redirects' key, under a user's hive root.
const SHELL_FOLDERS: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders";
/// `REG_APP_HIVE_OPEN_READ_ONLY` (`REG_OPEN_READ_ONLY`, winnt.h), which the `windows` crate
/// lacks: the app hive is opened for reading only, so the read writes nothing to the user's
/// `NTUSER.DAT`. Not verified on a VM that `RegLoadAppKeyW` takes it; a load it refuses is a
/// profile that can't be read, which the step treats as such (no unmapped VM is ended).
const APP_HIVE_READ_ONLY: u32 = 0x0000_2000;

/// An open key, closed on drop (an app hive is unloaded with its last key).
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful open or load and is closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// `call` failed with `code`, as a line for the log.
fn failed(call: &str, code: WIN32_ERROR) -> Failed {
    format!("{call} failed on a non-pool profile ({})", code.0)
}

/// Whether `code` is a success (`true`) or says nothing is there (`false`).
fn present(call: &str, code: WIN32_ERROR) -> Result<bool, Failed> {
    match code {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        other => Err(failed(call, other)),
    }
}

/// `<root>\<path>` opened to read; `None` when it isn't there.
fn open(root: HKEY, path: &str) -> Result<Option<Key>, Failed> {
    let subkey = HSTRING::from(path);
    let mut opened = HKEY::default();
    // SAFETY: the subkey is NUL-terminated; `opened` is a live out-pointer, closed by `Key`.
    let code = unsafe {
        RegOpenKeyExW(
            root,
            &subkey,
            None,
            KEY_READ | KEY_WOW64_64KEY,
            &raw mut opened,
        )
    };
    Ok(present("RegOpenKeyExW", code)?.then_some(Key(opened)))
}

/// Every SID `ProfileList` lists; a subkey whose name isn't a SID is passed over.
pub(super) fn profile_sids() -> Result<Vec<Sid>, Failed> {
    let key = open(HKEY_LOCAL_MACHINE, PROFILE_LIST)?
        .ok_or_else(|| "ProfileList isn't there".to_owned())?;
    let mut sids = Vec::new();
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
            return Err(failed("RegEnumKeyExW (ProfileList)", code));
        }
        let units = name
            .get(..usize::try_from(len).unwrap_or(0))
            .unwrap_or_default();
        if let Ok(sid) = Sid::parse(&String::from_utf16_lossy(units)) {
            sids.push(sid);
        }
    }
    Ok(sids)
}

/// Whether `user`'s profile holds `temp\<id>` under its `Local AppData`, as
/// [`super::VmStopper::non_pool_vm_folder`] says. No pool user can write these folders, so
/// links are followed.
pub(super) fn non_pool_vm_folder(user: &Sid, id: &VmId) -> Result<bool, Failed> {
    let profile = profile_of(user)
        .map_err(|_error| "a non-pool ProfileList entry couldn't be read".to_owned())?;
    let Some(profile) = profile else {
        return Ok(false);
    };
    let mut default = profile.path.join(r"AppData\Local\temp");
    default.push(id.as_str());
    if folder(&default)? {
        return Ok(true);
    }
    let Some(value) = redirect(user, &profile.path)? else {
        return Ok(false);
    };
    let root = profile
        .path
        .to_str()
        .ok_or_else(|| "a non-pool profile folder isn't text".to_owned())?;
    let local = local_app_data(&value, root)
        .ok_or_else(|| "a non-pool Local AppData redirect can't be expanded".to_owned())?;
    let mut redirected = PathBuf::from(local);
    redirected.push("temp");
    redirected.push(id.as_str());
    folder(&redirected)
}

/// Whether the folder `at` is there, following links; nothing there is `false`.
fn folder(at: &Path) -> Result<bool, Failed> {
    match std::fs::metadata(at) {
        Ok(found) => Ok(found.is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            let code = error.raw_os_error().unwrap_or(0);
            Err(format!("a non-pool profile couldn't be read ({code})"))
        }
    }
}

/// `user`'s `Local AppData` in `User Shell Folders`, unexpanded, from `HKU\<user>` when its hive
/// is loaded, else from `<profile>\NTUSER.DAT` loaded read-only; `None` when the value or its
/// key is missing, or the profile has no `NTUSER.DAT`.
fn redirect(user: &Sid, profile: &Path) -> Result<Option<String>, Failed> {
    if let Some(hive) = open(HKEY_USERS, user.as_str())? {
        return value(&hive);
    }
    let file = HSTRING::from(profile.join("NTUSER.DAT").as_os_str());
    let mut loaded = HKEY::default();
    let options = REG_PROCESS_APPKEY | APP_HIVE_READ_ONLY;
    // SAFETY: a NUL-terminated path and a live out-pointer, closed (and so unloaded) by `Key`.
    let code = unsafe { RegLoadAppKeyW(&file, &raw mut loaded, KEY_READ.0, options, None) };
    if !present("RegLoadAppKeyW", code)? {
        return Ok(None);
    }
    value(&Key(loaded))
}

/// `Local AppData` under `<hive>\<SHELL_FOLDERS>`, unexpanded; `None` when it or its key is
/// missing.
fn value(hive: &Key) -> Result<Option<String>, Failed> {
    let subkey = HSTRING::from(SHELL_FOLDERS);
    let name = w!("Local AppData");
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let mut size = 0u32;
    // SAFETY: NUL-terminated names; no data pointer asks only for the size.
    let code = unsafe {
        RegGetValueW(
            hive.0,
            &subkey,
            name,
            flags,
            None,
            None,
            Some(&raw mut size),
        )
    };
    if !present("RegGetValueW (Local AppData)", code)? {
        return Ok(None);
    }
    let mut buffer = vec![0u16; usize::try_from(size).unwrap_or(0).div_ceil(2)];
    // SAFETY: `buffer` is live, writable and at least `size` bytes long for the call.
    let code = unsafe {
        RegGetValueW(
            hive.0,
            &subkey,
            name,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if !present("RegGetValueW (Local AppData)", code)? {
        return Ok(None);
    }
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    let text = buffer.get(..end).unwrap_or_default();
    String::from_utf16(text)
        .map(Some)
        .map_err(|_error| "a non-pool Local AppData redirect isn't text".to_owned())
}
