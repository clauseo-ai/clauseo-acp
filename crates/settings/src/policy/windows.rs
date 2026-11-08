//! Windows' policy sources: the registry, then `ProgramData\Clauseo\policy.json`, read only
//! when an administrator owns it ([`acl`]). This module only collects the registry's raw values;
//! [`super::registry::parse`] reads them. It also reads Chrome's and Edge's
//! `RemoteDebuggingAllowed` for [`crate::chromium_policy`] ([`SystemRegistry`]).
#![expect(unsafe_code, reason = "FFI to the OS policy store")]

mod acl;
#[cfg(test)]
mod tests;

pub(super) use acl::trust;

use super::registry::{self, KEY, RegValue, SOURCE};
use super::{PolicyRead, PolicySource, TrustedFilePolicy};
use crate::SystemDirs;
use crate::chromium_policy::{Hive, Registry as ChromiumRegistry};
use crate::error::Error;
use crate::locations::{KnownFolder, PlatformDirs as _};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_DWORD,
    REG_EXPAND_SZ, REG_MULTI_SZ, REG_ROUTINE_FLAGS, REG_SZ, REG_VALUE_TYPE, RRF_RT_ANY,
    RRF_SUBKEY_WOW6464KEY, RegCloseKey, RegEnumValueW, RegGetValueW, RegOpenKeyExW,
    RegQueryInfoKeyW,
};
use windows::core::{HSTRING, PWSTR};

/// The registry, then the `policy.json` in the `ProgramData` known folder, read only when it
/// and `ProgramData\Clauseo` pass the trust check ([`TrustedFilePolicy`]).
pub(super) fn sources() -> Vec<Box<dyn PolicySource>> {
    let file: Box<dyn PolicySource> = match SystemDirs.known_folder(KnownFolder::ProgramData) {
        Some(data) => Box::new(TrustedFilePolicy::admin_owned(
            data.join("Clauseo").join("policy.json"),
        )),
        None => Box::new(NoProgramData),
    };
    vec![Box::new(Registry), file]
}

/// The policy key in `HKEY_LOCAL_MACHINE`.
struct Registry;

impl PolicySource for Registry {
    fn name(&self) -> &str {
        SOURCE
    }

    fn read(&self) -> PolicyRead {
        match values(KEY) {
            Ok(None) => PolicyRead::Absent,
            Ok(Some(values)) => PolicyRead::Read(registry::parse(values)),
            Err(error) => PolicyRead::Unreadable(error),
        }
    }
}

/// Stands in for the `policy.json` when Windows gives no `ProgramData` folder.
struct NoProgramData;

impl PolicySource for NoProgramData {
    fn name(&self) -> &'static str {
        r"ProgramData\Clauseo\policy.json"
    }

    fn read(&self) -> PolicyRead {
        PolicyRead::Unreadable(Error::NoFolder {
            folder: "ProgramData",
        })
    }
}

/// Every value under `HKLM\<key>`, through the 64-bit view; `None` when the key is missing.
pub(super) fn values(key: &str) -> Result<Option<Vec<(String, RegValue)>>, Error> {
    let fail = |code: u32| Error::Registry {
        key: key.to_owned(),
        code,
    };
    let mut handle = HKEY::default();
    // SAFETY: the subkey is a NUL-terminated wide string that outlives the call, and `handle`
    // is a valid place for the opened key.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(key),
            None,
            KEY_READ | KEY_WOW64_64KEY,
            &raw mut handle,
        )
    };
    if opened == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if opened != ERROR_SUCCESS {
        return Err(fail(opened.0));
    }
    let key = Key(handle);
    let (mut max_name, mut max_data) = (0u32, 0u32);
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
            Some(&raw mut max_data),
            None,
            None,
        )
    };
    if info != ERROR_SUCCESS {
        return Err(fail(info.0));
    }
    // The name length leaves out the NUL; one more for it. Data lengths are in bytes.
    let mut name = vec![0u16; max_name as usize + 1];
    let mut data = vec![0u8; max_data as usize];
    let mut values = Vec::new();
    for index in 0.. {
        let mut name_len = u32::try_from(name.len()).unwrap_or(u32::MAX);
        let mut data_len = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let mut kind = 0u32;
        // SAFETY: `name` and `data` are writable for the lengths passed, which the call reads
        // and then sets to what it wrote.
        let read = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut name_len,
                None,
                Some(&raw mut kind),
                Some(data.as_mut_ptr()),
                Some(&raw mut data_len),
            )
        };
        if read == ERROR_NO_MORE_ITEMS {
            break;
        }
        if read != ERROR_SUCCESS {
            return Err(fail(read.0));
        }
        let value_name = String::from_utf16_lossy(name.get(..name_len as usize).unwrap_or(&[]));
        let bytes = data.get(..data_len as usize).unwrap_or(&[]);
        values.push((value_name, reg_value(kind, bytes)));
    }
    Ok(Some(values))
}

/// A raw registry value as a [`RegValue`].
fn reg_value(kind: u32, bytes: &[u8]) -> RegValue {
    match kind {
        k if k == REG_DWORD.0 => match <[u8; 4]>::try_from(bytes) {
            Ok(word) => RegValue::Dword(u32::from_le_bytes(word)),
            Err(_) => RegValue::Other(kind),
        },
        k if k == REG_SZ.0 => {
            RegValue::Sz(wide_strings(bytes).into_iter().next().unwrap_or_default())
        }
        k if k == REG_MULTI_SZ.0 => RegValue::MultiSz(wide_strings(bytes)),
        _ => RegValue::Other(kind),
    }
}

/// The NUL-separated UTF-16 strings in `bytes`, up to the empty one that ends a `REG_MULTI_SZ`.
fn wide_strings(bytes: &[u8]) -> Vec<String> {
    let wide: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    wide.split(|&unit| unit == 0)
        .take_while(|part| !part.is_empty())
        .map(String::from_utf16_lossy)
        .collect()
}

/// An open registry key, closed on drop.
struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: the key was opened by RegOpenKeyExW and is closed only here. A failed close
        // leaves nothing to do.
        let _closed = unsafe { RegCloseKey(self.0) };
    }
}

/// This machine's registry, one value at a time, for [`crate::chromium_policy`]: a `REG_DWORD`, a
/// `REG_SZ` or `REG_EXPAND_SZ` (as Chrome reads both) of up to [`MAX_STRING_BYTES`], or the type
/// of anything else.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SystemRegistry;

impl ChromiumRegistry for SystemRegistry {
    fn value(&self, hive: Hive, key: &str, value: &str) -> Result<Option<RegValue>, u32> {
        let root = match hive {
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
            Hive::CurrentUser => HKEY_CURRENT_USER,
        };
        let mut kind = REG_VALUE_TYPE(0);
        let mut data = [0u8; MAX_STRING_BYTES];
        let mut len = u32::try_from(data.len()).unwrap_or(0);
        // SAFETY: the key and value names are NUL-terminated wide strings that outlive the call;
        // `kind` and `len` are valid places, and `data` is writable for the `len` bytes passed.
        let read = unsafe {
            RegGetValueW(
                root,
                &HSTRING::from(key),
                &HSTRING::from(value),
                REG_ROUTINE_FLAGS(RRF_RT_ANY.0 | RRF_SUBKEY_WOW6464KEY.0),
                Some(&raw mut kind),
                Some(data.as_mut_ptr().cast()),
                Some(&raw mut len),
            )
        };
        if crate::chromium_policy::registry_missing(read.0) {
            return Ok(None);
        }
        // Longer than any integer a string could hold: a bad value whatever its type.
        if read == ERROR_MORE_DATA {
            return Ok(Some(RegValue::Other(kind.0)));
        }
        if read != ERROR_SUCCESS {
            return Err(read.0);
        }
        let bytes = data.get(..len as usize).unwrap_or(&[]);
        let kind = if kind == REG_EXPAND_SZ { REG_SZ } else { kind };
        Ok(Some(match kind {
            REG_DWORD | REG_SZ => reg_value(kind.0, bytes),
            _ => RegValue::Other(kind.0),
        }))
    }
}

/// The most bytes of a Chromium policy string value read: an integer with room for spaces.
const MAX_STRING_BYTES: usize = 256;
