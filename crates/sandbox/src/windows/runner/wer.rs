//! The registry calls of turning WER off for the pool user on Windows (`wer_flow.rs` holds the
//! order and decision 0049, "Crash reporting (WER)", the why): keys under `HKEY_USERS\<pool
//! SID>`, each part opened with `REG_OPTION_OPEN_LINK` so a link opens as itself, a link told by
//! its `SymbolicLinkValue`, and links deleted as themselves (`NtDeleteKey` on the link's own
//! handle), never through. Also set-up's repair of a loaded pool hive ([`repair_hive`]).
#![expect(unsafe_code, reason = "FFI to the registry calls")]

use windows::Wdk::System::Registry::NtDeleteKey;
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS,
    ERROR_SUCCESS, ERROR_UNSUPPORTED_TYPE, HANDLE, WIN32_ERROR,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_CREATE_SUB_KEY, KEY_ENUMERATE_SUB_KEYS,
    KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_OPTION_OPEN_LINK,
    REG_SAM_FLAGS, REG_VALUE_TYPE, RRF_RT_REG_DWORD, RegCloseKey, RegCreateKeyExW, RegEnumKeyExW,
    RegGetKeySecurity, RegGetValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use super::wer_flow::{How, Registry, Root, repair};
use crate::windows::sids::Sid;

/// The value a registry link holds its target in.
const LINK_VALUE: PCWSTR = w!("SymbolicLinkValue");
/// `READ_CONTROL`.
const READ_CONTROL: u32 = 0x0002_0000;
/// `DELETE`.
const DELETE: u32 = 0x0001_0000;
/// A key name's longest form, in characters, with its NUL.
const NAME_CAP: usize = 256;

/// The registry as this process's token reaches it.
pub(crate) struct OsRegistry;

/// An open registry key, closed on drop.
pub(crate) struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: opened by RegCreateKeyExW or RegOpenKeyExW and closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

fn check(code: WIN32_ERROR) -> Result<(), u32> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(code.0)
    }
}

impl Registry for OsRegistry {
    type Key = Key;

    fn open(&mut self, parent: Option<&Key>, name: &str, how: How) -> Result<Option<Key>, u32> {
        let root = parent.map_or(HKEY_USERS, |key| key.0);
        let subkey = HSTRING::from(name);
        let access = match how {
            How::Write | How::Make => REG_SAM_FLAGS(
                KEY_QUERY_VALUE.0 | KEY_SET_VALUE.0 | KEY_CREATE_SUB_KEY.0 | READ_CONTROL,
            ),
            How::Delete => {
                REG_SAM_FLAGS(KEY_QUERY_VALUE.0 | KEY_ENUMERATE_SUB_KEYS.0 | READ_CONTROL | DELETE)
            }
        };
        let mut opened = HKEY::default();
        let code = if how == How::Make {
            // SAFETY: `subkey` is a live NUL-terminated string; no class, attributes or
            // disposition; `opened` is a live out-pointer, owned by `Key` once written.
            unsafe {
                RegCreateKeyExW(
                    root,
                    &subkey,
                    None,
                    PCWSTR::null(),
                    REG_OPTION_NON_VOLATILE | REG_OPTION_OPEN_LINK,
                    access,
                    None,
                    &raw mut opened,
                    None,
                )
            }
        } else {
            // SAFETY: as above; `REG_OPTION_OPEN_LINK` opens a link as itself.
            unsafe {
                RegOpenKeyExW(
                    root,
                    &subkey,
                    Some(REG_OPTION_OPEN_LINK.0),
                    access,
                    &raw mut opened,
                )
            }
        };
        match code {
            ERROR_SUCCESS => Ok(Some(Key(opened))),
            ERROR_FILE_NOT_FOUND if how != How::Make => Ok(None),
            other => Err(other.0),
        }
    }

    fn is_link(&mut self, key: &Key) -> Result<bool, u32> {
        // SAFETY: the key is open for queries; only whether the value exists is asked.
        let code = unsafe { RegQueryValueExW(key.0, LINK_VALUE, None, None, None, None) };
        match code {
            ERROR_SUCCESS => Ok(true),
            ERROR_FILE_NOT_FOUND => Ok(false),
            other => Err(other.0),
        }
    }

    fn security(&mut self, key: &Key) -> Result<Vec<u8>, u32> {
        let what = DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
        let mut size = 0u32;
        // SAFETY: no buffer asks only for the size, written to `size`.
        match unsafe { RegGetKeySecurity(key.0, what, None, &raw mut size) } {
            ERROR_INSUFFICIENT_BUFFER => {}
            other => return Err(other.0),
        }
        let mut buffer = vec![0u8; usize::try_from(size).unwrap_or(0)];
        let descriptor = PSECURITY_DESCRIPTOR(buffer.as_mut_ptr().cast());
        // SAFETY: the key is open with READ_CONTROL; `buffer` is writable for `size` bytes.
        check(unsafe { RegGetKeySecurity(key.0, what, Some(descriptor), &raw mut size) })?;
        buffer.truncate(usize::try_from(size).unwrap_or(0));
        Ok(buffer)
    }

    fn set_dword(&mut self, key: &Key, name: &str, value: u32) -> Result<(), u32> {
        let name = HSTRING::from(name);
        // SAFETY: the key is open for setting values; the data is the DWORD's four bytes.
        check(unsafe { RegSetValueExW(key.0, &name, None, REG_DWORD, Some(&value.to_le_bytes())) })
    }

    fn dword(&mut self, key: &Key, name: &str) -> Result<Option<u32>, u32> {
        let name = HSTRING::from(name);
        let mut kind = REG_VALUE_TYPE::default();
        let mut data = [0u8; 4];
        let mut size = u32::try_from(data.len()).unwrap_or(0);
        // SAFETY: the key is open for queries; `data` is writable for `size` bytes, and `kind`
        // and `size` are live out-pointers.
        let code = unsafe {
            RegQueryValueExW(
                key.0,
                &name,
                None,
                Some(&raw mut kind),
                Some(data.as_mut_ptr()),
                Some(&raw mut size),
            )
        };
        match code {
            ERROR_SUCCESS if kind == REG_DWORD && size == 4 => Ok(Some(u32::from_le_bytes(data))),
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND | ERROR_MORE_DATA => Ok(None),
            other => Err(other.0),
        }
    }

    fn read(&mut self, root: Root, path: &str, name: &str) -> Result<Option<u32>, u32> {
        let root = match root {
            Root::Users => HKEY_USERS,
            Root::Machine => HKEY_LOCAL_MACHINE,
        };
        let (path, name) = (HSTRING::from(path), HSTRING::from(name));
        let mut value = 0u32;
        let mut size = u32::try_from(size_of::<u32>()).unwrap_or(0);
        // SAFETY: both strings are live and NUL-terminated; `value` is writable for `size`
        // bytes.
        let code = unsafe {
            RegGetValueW(
                root,
                &path,
                &name,
                RRF_RT_REG_DWORD,
                None,
                Some((&raw mut value).cast()),
                Some(&raw mut size),
            )
        };
        match code {
            ERROR_SUCCESS => Ok(Some(value)),
            ERROR_FILE_NOT_FOUND | ERROR_UNSUPPORTED_TYPE | ERROR_MORE_DATA => Ok(None),
            other => Err(other.0),
        }
    }

    fn delete(&mut self, key: Key) -> Result<(), u32> {
        if !self.is_link(&key)? {
            for name in subkeys(&key)? {
                if let Some(child) = self.open(Some(&key), &name, How::Delete)? {
                    self.delete(child)?;
                }
            }
        }
        // SAFETY: the handle is this key's own (a link's, opened as itself), open for DELETE.
        let status = unsafe { NtDeleteKey(HANDLE(key.0.0)) };
        if status.is_err() {
            return Err(u32::from_ne_bytes(status.0.to_ne_bytes()));
        }
        Ok(())
    }
}

/// The names of `key`'s subkeys.
fn subkeys(key: &Key) -> Result<Vec<String>, u32> {
    let mut names = Vec::new();
    let mut buffer = [0u16; NAME_CAP];
    for index in 0u32.. {
        let mut size = u32::try_from(buffer.len()).unwrap_or(0);
        // SAFETY: the key is open to enumerate; `buffer` is writable for `size` characters.
        let code = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(buffer.as_mut_ptr())),
                &raw mut size,
                None,
                None,
                None,
                None,
            )
        };
        match code {
            ERROR_SUCCESS => {
                let end = usize::try_from(size).unwrap_or(0).min(buffer.len());
                names.push(String::from_utf16_lossy(buffer.get(..end).unwrap_or(&[])));
            }
            ERROR_NO_MORE_ITEMS => break,
            other => return Err(other.0),
        }
    }
    Ok(names)
}

/// Set-up's repair of `pool`'s hive while its profile is loaded (`setup/logon.rs`): deletes a
/// link on the way to the WER key, or a WER key that is a link or unsafe (`wer_flow::repair`),
/// so the next launch makes it again. Whether it deleted anything.
pub(crate) fn repair_hive(pool: &Sid) -> Result<bool, String> {
    repair(&mut OsRegistry, pool).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
