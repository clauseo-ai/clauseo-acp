//! Defender's Controlled folder access on Windows, read in this process from the registry places
//! its setting is kept in (Defender has no documented API for it; `Get-MpPreference` reads the
//! same places through WMI), and the folders it protects by default, from the known-folder lookup
//! the folder chooser already makes. No child process is started, so no console shows and nothing
//! is left running when the app ends.
#![expect(unsafe_code, reason = "FFI to the Win32 registry")]

use std::path::PathBuf;

use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_ROUTINE_FLAGS, RRF_RT_REG_DWORD,
    RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegEnumValueW, RegGetValueW, RegOpenKeyExW,
    RegQueryInfoKeyW,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Favorites, FOLDERID_Music, FOLDERID_Pictures,
    FOLDERID_Videos,
};
use windows::core::{HSTRING, PWSTR};

use super::CfaLayer;

/// Group policy's place (Windows Components › Microsoft Defender Antivirus › Microsoft Defender
/// Exploit Guard › Controlled folder access).
const POLICY: &str = "SOFTWARE\\Policies\\Microsoft\\Windows Defender\\Windows Defender Exploit \
                      Guard\\Controlled Folder Access";
/// MDM's (Intune's Defender policy CSP).
const MDM: &str = "SOFTWARE\\Microsoft\\PolicyManager\\current\\device\\Defender";
/// Defender's own, which Windows Security's switch writes.
const LOCAL: &str = "SOFTWARE\\Microsoft\\Windows Defender\\Windows Defender Exploit \
                     Guard\\Controlled Folder Access";
/// The on/off value in each place: 0 off, 1 on (blocking), 2 audit, 3 and 4 disks only.
const ENABLE: &str = "EnableControlledFolderAccess";
/// The subkey of the group policy and local places whose value names are the added folders.
const FOLDERS: &str = "ProtectedFolders";
/// MDM's added folders, one string, separated by U+F000.
const MDM_FOLDERS: &str = "ControlledFolderAccessProtectedFolders";

/// Each place's `EnableControlledFolderAccess`, most specific first, and the folders the places
/// add to Windows' defaults (those that could be read).
pub(super) fn cfa_layers() -> (Vec<CfaLayer>, Vec<PathBuf>) {
    let layers: Vec<CfaLayer> = [POLICY, MDM, LOCAL]
        .into_iter()
        .map(|key| match dword(key, ENABLE) {
            Ok(Some(enable)) => CfaLayer::Set(enable),
            Ok(None) => CfaLayer::Unset,
            Err(code) => {
                tracing::info!(
                    key,
                    code = code.0,
                    "permissions: a Defender key did not read"
                );
                CfaLayer::Unreadable
            }
        })
        .collect();
    let mut protected = Vec::new();
    for key in [POLICY, LOCAL] {
        protected.extend(value_names(&format!("{key}\\{FOLDERS}")).map(PathBuf::from));
    }
    protected.extend(
        string(MDM, MDM_FOLDERS)
            .unwrap_or_default()
            .split('\u{F000}')
            .filter(|folder| !folder.is_empty())
            .map(PathBuf::from),
    );
    tracing::info!(
        ?layers,
        added = protected.len(),
        "permissions: Controlled folder access read"
    );
    (layers, protected)
}

/// Whether `code` says the key or the value is not there.
fn missing(code: WIN32_ERROR) -> bool {
    code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND
}

/// `HKLM\<key>`'s DWORD `value` in the 64-bit view; `None` when the key or the value is missing,
/// the error when it does not read (access denied, another type).
fn dword(key: &str, value: &str) -> Result<Option<u32>, WIN32_ERROR> {
    let mut data = 0u32;
    let mut size: u32 = 4;
    // SAFETY: the key and value names are NUL-terminated wide strings that outlive the call, and
    // `data` is `size` writable bytes.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(key),
            &HSTRING::from(value),
            REG_ROUTINE_FLAGS(RRF_RT_REG_DWORD.0 | RRF_SUBKEY_WOW6464KEY.0),
            None,
            Some((&raw mut data).cast()),
            Some(&raw mut size),
        )
    };
    match read {
        ERROR_SUCCESS => Ok(Some(data)),
        code if missing(code) => Ok(None),
        code => Err(code),
    }
}

/// `HKLM\<key>`'s string `value` in the 64-bit view; `None` when it is missing or does not read.
fn string(key: &str, value: &str) -> Option<String> {
    let (key, value) = (HSTRING::from(key), HSTRING::from(value));
    let flags = REG_ROUTINE_FLAGS(RRF_RT_REG_SZ.0 | RRF_SUBKEY_WOW6464KEY.0);
    let mut size = 0u32;
    // SAFETY: as below, with no buffer: the call only sets the size it needs, in bytes.
    let sized = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &key,
            &value,
            flags,
            None,
            None,
            Some(&raw mut size),
        )
    };
    if sized != ERROR_SUCCESS {
        return None;
    }
    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    // SAFETY: the names outlive the call and `buffer` is `size` writable bytes.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &key,
            &value,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if read != ERROR_SUCCESS {
        return None;
    }
    let units = (size as usize / 2).min(buffer.len());
    let text = buffer.get(..units).unwrap_or_default();
    let end = text
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(text.len());
    Some(String::from_utf16_lossy(
        text.get(..end).unwrap_or_default(),
    ))
}

/// An open key, closed when dropped.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by RegOpenKeyExW and is closed once, here. A failed close
        // leaves nothing to do.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// The value names under `HKLM\<key>` in the 64-bit view; none when it is missing or does not
/// read.
fn value_names(key: &str) -> Vec<String> {
    let mut handle = HKEY::default();
    // SAFETY: the subkey is a NUL-terminated wide string that outlives the call, and `handle` is
    // a valid place for the opened key.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(key),
            None,
            KEY_READ | KEY_WOW64_64KEY,
            &raw mut handle,
        )
    };
    if opened != ERROR_SUCCESS {
        return Vec::new();
    }
    let key = Key(handle);
    let mut max_name = 0u32;
    // SAFETY: `key.0` is open, and every out-pointer is either `None` or a valid `u32`.
    let info = unsafe {
        RegQueryInfoKeyW(
            key.0,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&raw mut max_name),
            None,
            None,
            None,
        )
    };
    if info != ERROR_SUCCESS {
        return Vec::new();
    }
    // The name length leaves out the NUL; one more for it.
    let mut name = vec![0u16; max_name as usize + 1];
    let mut names = Vec::new();
    for index in 0.. {
        let mut name_len = u32::try_from(name.len()).unwrap_or(u32::MAX);
        // SAFETY: `name` is writable for the length passed, which the call reads and then sets
        // to what it wrote; no data is asked for.
        let read = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut name_len,
                None,
                None,
                None,
                None,
            )
        };
        // ERROR_NO_MORE_ITEMS at the end; any other error ends the list there too.
        if read != ERROR_SUCCESS {
            break;
        }
        let written = name.get(..name_len as usize).unwrap_or_default();
        names.push(String::from_utf16_lossy(written));
    }
    names
}

/// The folders Controlled folder access protects whatever its list says: the user's Documents,
/// Desktop, Pictures, Videos, Music and Favorites, those Windows gives.
pub(super) fn protected_defaults() -> Vec<PathBuf> {
    [
        &FOLDERID_Documents,
        &FOLDERID_Desktop,
        &FOLDERID_Pictures,
        &FOLDERID_Videos,
        &FOLDERID_Music,
        &FOLDERID_Favorites,
    ]
    .into_iter()
    .filter_map(crate::chooser::known_folder)
    .map(PathBuf::from)
    .collect()
}
