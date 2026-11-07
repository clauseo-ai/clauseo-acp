//! Windows: the value written under `HKCU`.
#![expect(unsafe_code, reason = "FFI to the Win32 registry")]

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

/// The value's name.
const VALUE: &str = "UiLanguage";

/// Sets `HKCU\<key>\`[`VALUE`] to `tag` as `REG_SZ`, making the key if it is missing. `Err`
/// is the failed call's Win32 code.
pub(super) fn write_value(key: &str, tag: &str) -> Result<(), u32> {
    let subkey = HSTRING::from(key);
    let mut opened = HKEY::default();
    // SAFETY: the subkey is a NUL-terminated HSTRING alive for the call and `opened` a live
    // out-pointer; no class, security attributes or disposition.
    let made = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &subkey,
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &raw mut opened,
            None,
        )
    };
    if made != ERROR_SUCCESS {
        return Err(made.0);
    }
    // REG_SZ data: UTF-16LE with its terminating NUL.
    let data: Vec<u8> = tag
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect();
    let name = HSTRING::from(VALUE);
    // SAFETY: `opened` is open with KEY_SET_VALUE; the name is NUL-terminated and `data` a live
    // slice whose length the binding passes.
    let set = unsafe { RegSetValueExW(opened, &name, None, REG_SZ, Some(&data)) };
    // SAFETY: `opened` was opened above and is closed once, here. A failed close leaves nothing
    // to do.
    let _closed = unsafe { RegCloseKey(opened) };
    if set == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(set.0)
    }
}

/// `HKCU\<key>\`[`VALUE`] as a string, `None` when it cannot be read.
#[cfg(test)]
pub(super) fn read_value(key: &str) -> Option<String> {
    use windows::Win32::System::Registry::{RRF_RT_REG_SZ, RegGetValueW};
    let subkey = HSTRING::from(key);
    let name = HSTRING::from(VALUE);
    let mut buffer = [0u16; 64];
    let mut size = u32::try_from(size_of_val(&buffer)).ok()?;
    // SAFETY: subkey and name are NUL-terminated HSTRINGs; `buffer` is live and `size` holds its
    // length in bytes, which the call does not write past.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &subkey,
            &name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if read != ERROR_SUCCESS {
        return None;
    }
    let units = usize::try_from(size).ok()? / 2;
    let text = buffer.get(..units)?;
    Some(
        String::from_utf16_lossy(text)
            .trim_end_matches('\0')
            .to_owned(),
    )
}

/// Deletes `HKCU\<key>` and its values.
#[cfg(test)]
pub(super) fn delete_key(key: &str) -> Result<(), u32> {
    use windows::Win32::System::Registry::RegDeleteTreeW;
    let subkey = HSTRING::from(key);
    // SAFETY: the subkey is a NUL-terminated HSTRING alive for the call.
    let deleted = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &subkey) };
    if deleted == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(deleted.0)
    }
}
