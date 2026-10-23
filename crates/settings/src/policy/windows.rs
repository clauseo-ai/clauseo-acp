//! Windows' policy sources: the registry, then `ProgramData\Clauseo\policy.json`. This module
//! only collects the registry's raw values; [`super::registry::parse`] reads them.
#![expect(unsafe_code, reason = "FFI to the OS policy store")]

#[cfg(test)]
mod tests;

use super::registry::{self, KEY, RegValue, SOURCE};
use super::{FilePolicy, PolicyRead, PolicySource};
use crate::error::Error;
use crate::locations::{KnownFolder, PlatformDirs as _};
use crate::{SystemDirs, SystemFiles};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_DWORD, REG_MULTI_SZ, REG_SZ,
    RegCloseKey, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW,
};
use windows::core::{HSTRING, PWSTR};

/// The registry, then the `policy.json` in the `ProgramData` known folder.
pub(super) fn sources() -> Vec<Box<dyn PolicySource>> {
    let file: Box<dyn PolicySource> = match SystemDirs.known_folder(KnownFolder::ProgramData) {
        Some(data) => Box::new(FilePolicy::new(
            data.join("Clauseo").join("policy.json"),
            SystemFiles,
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
