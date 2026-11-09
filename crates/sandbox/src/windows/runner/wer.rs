//! The registry calls of turning WER off for the pool user on Windows (`wer_flow.rs` holds the
//! order and decision 0049, "Crash reporting (WER)", the why): keys under the pool user's own
//! `HKEY_CURRENT_USER`, opened with `REG_OPTION_OPEN_LINK` so a link opens as itself, and a link
//! told by its `SymbolicLinkValue`.
#![expect(unsafe_code, reason = "FFI to the registry calls")]

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE,
    REG_OPTION_OPEN_LINK, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegQueryValueExW,
    RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::wer_flow::Registry;

/// The value a registry link holds its target in.
const LINK_VALUE: PCWSTR = w!("SymbolicLinkValue");

/// This user's `HKEY_CURRENT_USER`.
pub(super) struct OsRegistry;

/// An open registry key, closed on drop.
pub(super) struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: made by RegCreateKeyExW and closed once, here.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

impl Registry for OsRegistry {
    type Key = Key;

    fn open(&mut self, path: &str) -> Result<Key, u32> {
        let subkey = HSTRING::from(path);
        let mut opened = HKEY::default();
        // SAFETY: `subkey` is a live NUL-terminated string; no class, attributes or
        // disposition; `opened` is a live out-pointer, owned by `Key` once written.
        let code = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                &subkey,
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE | REG_OPTION_OPEN_LINK,
                KEY_SET_VALUE | KEY_QUERY_VALUE,
                None,
                &raw mut opened,
                None,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(code.0);
        }
        Ok(Key(opened))
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

    fn set_dword(&mut self, key: &Key, name: &str, value: u32) -> Result<(), u32> {
        let name = HSTRING::from(name);
        // SAFETY: the key is open for setting values; the data is the DWORD's four bytes.
        let code =
            unsafe { RegSetValueExW(key.0, &name, None, REG_DWORD, Some(&value.to_le_bytes())) };
        if code != ERROR_SUCCESS {
            return Err(code.0);
        }
        Ok(())
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
}

#[cfg(test)]
mod tests;
