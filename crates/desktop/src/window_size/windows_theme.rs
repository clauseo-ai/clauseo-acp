//! Windows: the app theme the OS gives apps, read before a window exists.
#![expect(unsafe_code, reason = "FFI to the Win32 registry")]

use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::w;

/// Whether Windows' app theme is dark: `HKCU\...\Themes\Personalize\AppsUseLightTheme` is `0`
/// (Settings, Personalisation, Colours, "Choose your app mode"). Light when the value is missing
/// or unreadable, as Windows itself treats it. One registry read, no file.
pub(super) fn apps_dark() -> bool {
    let mut value = 0u32;
    let mut size = u32::try_from(size_of_val(&value)).unwrap_or(4);
    // SAFETY: the subkey and name are NUL-terminated wide literals; `value` is a live u32 and
    // `size` holds its length in bytes, which RRF_RT_REG_DWORD does not write past.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&raw mut size),
        )
    };
    read == ERROR_SUCCESS && value == 0
}
