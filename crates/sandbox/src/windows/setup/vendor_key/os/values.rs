//! A key's values read and written whole, for the vendor key's walk, which copies `Sandbox`'s
//! values through its delete and remake.
#![expect(unsafe_code, reason = "FFI to the registry's value calls")]

use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
use windows::Win32::System::Registry::{REG_VALUE_TYPE, RegEnumValueW, RegSetValueExW};
use windows::core::{HSTRING, PWSTR};

use super::{Key, check};
use crate::windows::setup::vendor_key::{Miss, Value};

/// The values of the key `key` is open on, each whole (`RegEnumValueW`).
pub(super) fn read(key: &Key) -> Result<Vec<Value>, Miss> {
    let mut values = Vec::new();
    for index in 0u32.. {
        // A value's name is at most 16,383 characters.
        let mut name = vec![0u16; 16_384];
        let mut name_len = 16_384u32;
        let mut kind = 0u32;
        let mut size = 0u32;
        // SAFETY: `name` is live and `name_len` units long; the other out-pointers are live;
        // no data is asked for, only its size.
        let sized = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut name_len,
                None,
                Some(&raw mut kind),
                None,
                Some(&raw mut size),
            )
        };
        if sized == ERROR_NO_MORE_ITEMS {
            break;
        }
        check("RegEnumValueW", sized)?;
        let mut data = vec![0u8; usize::try_from(size).unwrap_or(0)];
        name_len = 16_384;
        // SAFETY: as above, and `data` is live and `size` bytes long.
        let read = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &raw mut name_len,
                None,
                Some(&raw mut kind),
                Some(data.as_mut_ptr()),
                Some(&raw mut size),
            )
        };
        check("RegEnumValueW", read)?;
        data.truncate(usize::try_from(size).unwrap_or(0));
        let units = name
            .get(..usize::try_from(name_len).unwrap_or(0))
            .unwrap_or_default();
        values.push(Value {
            name: String::from_utf16_lossy(units),
            kind,
            data,
        });
    }
    Ok(values)
}

/// Writes `values` to the key `key` is open on, each whole (`RegSetValueExW`).
pub(super) fn write(key: &Key, values: &[Value]) -> Result<(), Miss> {
    for value in values {
        let name = HSTRING::from(value.name.as_str());
        // SAFETY: the key is open with full access; the name is NUL-terminated (empty for
        // the default value); `data` is a live slice.
        let set = unsafe {
            RegSetValueExW(
                key.0,
                &name,
                None,
                REG_VALUE_TYPE(value.kind),
                Some(&value.data),
            )
        };
        check("RegSetValueExW", set)?;
    }
    Ok(())
}
