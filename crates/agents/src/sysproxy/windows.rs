//! Windows' Internet Settings, read with `RegGetValueW` (the `windows` crate). Two values of one
//! key in `HKEY_CURRENT_USER`; nothing is written.
#![expect(unsafe_code, reason = "FFI to the registry's RegGetValueW")]

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::core::HSTRING;

/// The key holding the static proxy.
const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
/// The most UTF-16 units of `ProxyServer` read.
const SERVER_MAX: usize = 4096;

/// `ProxyEnable` (0 when missing) and `ProxyServer`, or `None` when there is no server.
pub(super) fn internet_settings() -> Option<(u32, String)> {
    let key = HSTRING::from(KEY);
    let mut enable = 0u32;
    let mut size = u32::try_from(size_of::<u32>()).unwrap_or(4);
    // SAFETY: the key and value names are NUL-terminated wide strings that outlive the call;
    // `enable` is a valid, writable u32 and `size` holds its size in bytes, as RRF_RT_REG_DWORD
    // requires.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &HSTRING::from("ProxyEnable"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut enable).cast()),
            Some(&raw mut size),
        )
    };
    if read != ERROR_SUCCESS {
        enable = 0;
    }
    let mut buf = vec![0u16; SERVER_MAX];
    let mut size = u32::try_from(buf.len() * 2).unwrap_or(0);
    // SAFETY: as above; `buf` is a writable buffer of `size` bytes, and RegGetValueW writes at
    // most that many, NUL-terminated, or fails with ERROR_MORE_DATA and writes nothing usable.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &HSTRING::from("ProxyServer"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if read != ERROR_SUCCESS {
        return None;
    }
    let units = usize::try_from(size).unwrap_or(0) / 2;
    let text = buf.get(..units.min(buf.len())).unwrap_or_default();
    let end = text
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(text.len());
    let server = String::from_utf16_lossy(text.get(..end).unwrap_or_default());
    Some((enable, server))
}
