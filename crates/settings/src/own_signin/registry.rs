//! The registry's side of Claude Code's OS policy tiers: the `Settings` value of
//! `SOFTWARE\Policies\ClaudeCode` in HKLM and HKCU, through the 64-bit view as `reg.exe` sees
//! it, read-only. The host's offline probe reads them too (`offline/probe/settings/os_policy`).

#![expect(
    unsafe_code,
    reason = "RegGetValueW, the Win32 call the windows crate binds; its names and buffer live \
              across it"
)]

use windows::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_EXPAND_SZ, REG_ROUTINE_FLAGS, REG_SZ,
    REG_VALUE_TYPE, RRF_NOEXPAND, RRF_RT_ANY, RRF_SUBKEY_WOW6464KEY,
};
use windows::core::HSTRING;

use super::sources::{MAX_FILE, TierRead};

/// The key Claude Code's registry policy sits under, in HKLM and in HKCU.
const KEY: &str = r"SOFTWARE\Policies\ClaudeCode";

/// The value holding its settings document.
const VALUE: &str = "Settings";

/// The `Settings` values, HKLM's then HKCU's.
pub(crate) fn settings_values() -> Vec<TierRead> {
    vec![value(HKEY_LOCAL_MACHINE), value(HKEY_CURRENT_USER)]
}

/// `hive`'s `Settings` value: its text when a `REG_SZ` or `REG_EXPAND_SZ` (unexpanded, as
/// `reg.exe` prints it), `None` when it, its key or a string type is not there (Claude Code
/// reads no other type), `Err` when it cannot be read or is over [`MAX_FILE`]
/// UTF-16 units.
fn value(hive: HKEY) -> TierRead {
    let (mut kind, mut len) = (REG_VALUE_TYPE(0), 0u32);
    let sized = get(hive, &mut kind, None, &mut len);
    if missing(sized) {
        return Ok(None);
    }
    if sized != ERROR_SUCCESS {
        return Err(());
    }
    if kind != REG_SZ && kind != REG_EXPAND_SZ {
        return Ok(None);
    }
    let units = usize::try_from(len).map_err(drop)?.div_ceil(2);
    if u64::try_from(units).map_or(true, |units| units > MAX_FILE) {
        return Err(());
    }
    let mut data = vec![0u16; units.max(1)];
    let read = get(hive, &mut kind, Some(&mut data), &mut len);
    if missing(read) {
        return Ok(None);
    }
    // Changed between the two reads (`ERROR_MORE_DATA`, or a type no longer a string).
    if read != ERROR_SUCCESS || (kind != REG_SZ && kind != REG_EXPAND_SZ) {
        return Err(());
    }
    let written = usize::try_from(len).map_err(drop)? / 2;
    let text = data.get(..written).ok_or(())?;
    let text = String::from_utf16(text).map_err(drop)?;
    Ok(Some(text.trim_end_matches('\0').to_owned()))
}

/// One `RegGetValueW` of `hive`'s `Settings` into `data` (only its size and type without),
/// `len` its size in bytes going in and the bytes written coming out.
fn get(
    hive: HKEY,
    kind: &mut REG_VALUE_TYPE,
    data: Option<&mut [u16]>,
    len: &mut u32,
) -> WIN32_ERROR {
    let buffer = data.map(|data| {
        *len = u32::try_from(data.len().saturating_mul(2)).unwrap_or(u32::MAX);
        data.as_mut_ptr().cast()
    });
    let flags = REG_ROUTINE_FLAGS(RRF_RT_ANY.0 | RRF_NOEXPAND.0 | RRF_SUBKEY_WOW6464KEY.0);
    // SAFETY: the key and value names are NUL-terminated wide strings that outlive the call;
    // `kind` and `len` are valid places, and `buffer`, when given, is writable for `len` bytes.
    unsafe {
        windows::Win32::System::Registry::RegGetValueW(
            hive,
            &HSTRING::from(KEY),
            &HSTRING::from(VALUE),
            flags,
            Some(&raw mut *kind),
            buffer,
            Some(&raw mut *len),
        )
    }
}

/// Whether a read's error says the key or the value is not there.
fn missing(code: WIN32_ERROR) -> bool {
    crate::chromium_policy::registry_missing(code.0)
}
