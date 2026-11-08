//! The WER values on a real machine, under a scratch key of this user's own
//! `HKEY_CURRENT_USER`, never its real WER settings.
#![expect(unsafe_code, reason = "FFI to the registry in tests")]

use windows::Win32::System::Registry::{RRF_RT_REG_DWORD, RegDeleteTreeW, RegGetValueW};

use super::*;

/// The DWORD `name` under `HKEY_CURRENT_USER\<key>`, or the call's error.
fn dword(key: &str, name: &str) -> Result<u32, WIN32_ERROR> {
    let (key, name) = (HSTRING::from(key), HSTRING::from(name));
    let mut value = 0u32;
    let mut size = u32::try_from(size_of::<u32>()).unwrap();
    // SAFETY: both strings are live and NUL-terminated; `value` is writable for `size` bytes.
    let code = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &name,
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&raw mut size),
        )
    };
    if code == ERROR_SUCCESS {
        Ok(value)
    } else {
        Err(code)
    }
}

#[test]
fn the_wer_key_is_the_per_user_one_and_both_values_are_one() {
    assert_eq!(
        WER_KEY,
        r"Software\Microsoft\Windows\Windows Error Reporting"
    );
    assert_eq!(WER_VALUES, [("Disabled", 1), ("DontShowUI", 1)]);
}

#[test]
fn quiet_makes_the_key_sets_both_values_and_is_idempotent() {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    let scratch = format!(
        "Software\\ClauseoTest-{}",
        crate::windows::pipe::hex(&random)
    );
    let key = format!("{scratch}\\Windows Error Reporting");
    let first = quiet(&key);
    let second = quiet(&key);
    let values: Vec<_> = WER_VALUES
        .iter()
        .map(|(name, _)| dword(&key, name))
        .collect();
    let wide = HSTRING::from(scratch.as_str());
    // SAFETY: a live NUL-terminated path under this user's own hive, made by this test.
    let removed = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &wide) };
    first.unwrap();
    second.unwrap();
    assert_eq!(values, [Ok(1), Ok(1)]);
    assert_eq!(removed, ERROR_SUCCESS);
}

#[test]
fn a_failure_is_logged_with_its_code() {
    let line = failed_line(&io::Error::from_raw_os_error(5));
    assert!(line.ends_with("Windows Error Reporting for the sandbox user failed (5)"));
}
