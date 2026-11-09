//! The WER registry calls on a real machine, under a scratch key of this user's own
//! `HKEY_CURRENT_USER`, never its real WER settings: the values set and read back, twice, and a
//! key that is a registry link refused with nothing written through it.
#![expect(unsafe_code, reason = "FFI to the registry in tests")]

use windows::Wdk::System::Registry::NtDeleteKey;
use windows::Win32::Foundation::{HANDLE, WIN32_ERROR};
use windows::Win32::System::Registry::{
    KEY_ALL_ACCESS, KEY_CREATE_LINK, REG_LINK, REG_OPTION_CREATE_LINK, REG_OPTION_VOLATILE,
    REG_SAM_FLAGS, RegDeleteTreeW, RegOpenKeyExW,
};

use super::super::wer_flow::{Setting, WER_SETTINGS, WerError, apply};
use super::*;
use crate::windows::token::os::{own_token, user};

/// The standard `DELETE` right.
const DELETE: u32 = 0x0001_0000;

/// A fresh `Software\ClauseoTest-<hex>` path.
fn scratch() -> String {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    format!(
        "Software\\ClauseoTest-{}",
        crate::windows::pipe::hex(&random)
    )
}

/// [`WER_SETTINGS`]' values under `key` instead of the real WER key.
fn setting_at(key: String) -> Setting {
    Setting {
        key: Box::leak(key.into_boxed_str()),
        values: WER_SETTINGS[0].values,
    }
}

fn remove_tree(path: &str) -> WIN32_ERROR {
    let wide = HSTRING::from(path);
    // SAFETY: a live NUL-terminated path under this user's own hive, made by the test.
    unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &wide) }
}

#[test]
fn the_values_are_set_and_read_back_and_again_change_nothing() {
    let scratch = scratch();
    let setting = setting_at(format!("{scratch}\\Windows Error Reporting"));
    let first = apply(&mut OsRegistry, &setting);
    let second = apply(&mut OsRegistry, &setting);
    let read = OsRegistry.open(setting.key).map(|key| {
        setting
            .values
            .iter()
            .map(|(name, _)| OsRegistry.dword(&key, name))
            .collect::<Vec<_>>()
    });
    let removed = remove_tree(&scratch);
    assert_eq!(first, Ok(()));
    assert_eq!(second, Ok(()));
    assert_eq!(read, Ok(vec![Ok(Some(1)), Ok(Some(1))]));
    assert_eq!(removed, ERROR_SUCCESS);
}

/// Makes `HKEY_CURRENT_USER\<link>` a registry link to `\REGISTRY\USER\<sid>\<target>`.
fn plant_link(link: &str, target: &str) -> WIN32_ERROR {
    let sid = user(&own_token().unwrap()).unwrap();
    let to: Vec<u8> = format!("\\REGISTRY\\USER\\{}\\{target}", sid.as_str())
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let path = HSTRING::from(link);
    let mut key = HKEY::default();
    // SAFETY: the path is live and NUL-terminated; `key` is a live out-pointer, closed below.
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &path,
            None,
            PCWSTR::null(),
            REG_OPTION_CREATE_LINK | REG_OPTION_VOLATILE,
            KEY_ALL_ACCESS | KEY_CREATE_LINK,
            None,
            &raw mut key,
            None,
        )
    };
    if code != ERROR_SUCCESS {
        return code;
    }
    let key = Key(key);
    // SAFETY: the link key is open for setting values; a link's target has no NUL.
    unsafe { RegSetValueExW(key.0, LINK_VALUE, None, REG_LINK, Some(&to)) }
}

/// Deletes the link itself (opened with `REG_OPTION_OPEN_LINK`), not its target.
fn remove_link(link: &str) {
    let path = HSTRING::from(link);
    let mut key = HKEY::default();
    // SAFETY: the path is live and NUL-terminated; `key` is a live out-pointer, closed below.
    let code = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &path,
            Some(REG_OPTION_OPEN_LINK.0),
            REG_SAM_FLAGS(DELETE),
            &raw mut key,
        )
    };
    if code == ERROR_SUCCESS {
        let key = Key(key);
        // SAFETY: the handle is the link key's, open for DELETE.
        let _deleted = unsafe { NtDeleteKey(HANDLE(key.0.0)) };
    }
}

#[test]
fn a_key_that_is_a_registry_link_is_refused_and_its_target_left_alone() {
    let scratch = scratch();
    let target = format!("{scratch}\\target");
    let link = format!("{scratch}\\Windows Error Reporting");
    let made = OsRegistry.open(&target).map(drop);
    let planted = plant_link(&link, &target);
    let setting = setting_at(link.clone());
    let applied = apply(&mut OsRegistry, &setting);
    let through = OsRegistry
        .open(&target)
        .map(|key| OsRegistry.dword(&key, "Disabled"));
    remove_link(&link);
    let removed = remove_tree(&scratch);
    assert_eq!(made, Ok(()));
    assert_eq!(planted, ERROR_SUCCESS);
    assert_eq!(applied, Err(WerError::Link { key: setting.key }));
    assert_eq!(through, Ok(Ok(None)));
    assert_eq!(removed, ERROR_SUCCESS);
}
