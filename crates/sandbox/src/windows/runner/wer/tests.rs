//! The WER registry calls on a real machine, under a scratch key of this user's own hive
//! (`HKEY_USERS\<this user's SID>\Software\ClauseoTest-*`), never its real WER settings: the
//! values set and read back, twice; a key that is a registry link refused with nothing written
//! through it; and set-up's repair deleting that link and not its target.
#![expect(unsafe_code, reason = "FFI to the registry in tests")]

use windows::Win32::System::Registry::{
    KEY_ALL_ACCESS, KEY_CREATE_LINK, REG_LINK, REG_OPTION_CREATE_LINK, REG_OPTION_VOLATILE,
    RegDeleteTreeW,
};

use super::super::wer_flow::{Setting, WER_SETTINGS, WerError, apply, repair_one};
use super::*;
use crate::windows::token::os::{own_token, user};

fn me() -> Sid {
    user(&own_token().unwrap()).unwrap()
}

/// A fresh `ClauseoTest-<hex>` name.
fn scratch() -> &'static str {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    let name = format!("ClauseoTest-{}", crate::windows::pipe::hex(&random));
    Box::leak(name.into_boxed_str())
}

/// [`WER_SETTINGS`]' values at `Software\<scratch>\<last>` instead of the real WER key.
fn setting_at(scratch: &'static str, last: &'static str) -> Setting {
    Setting {
        path: Box::leak(vec!["Software", scratch, last].into_boxed_slice()),
        values: WER_SETTINGS[0].values,
    }
}

fn under_me(path: &str) -> HSTRING {
    HSTRING::from(format!("{}\\{path}", me().as_str()))
}

fn remove_tree(scratch: &str) -> WIN32_ERROR {
    let path = under_me(&format!("Software\\{scratch}"));
    // SAFETY: a live NUL-terminated path under this user's own hive, made by the test.
    unsafe { RegDeleteTreeW(HKEY_USERS, &path) }
}

#[test]
fn the_values_are_set_and_read_back_and_again_change_nothing() {
    let scratch = scratch();
    let setting = setting_at(scratch, "Windows Error Reporting");
    let first = apply(&mut OsRegistry, &me(), &setting);
    let second = apply(&mut OsRegistry, &me(), &setting);
    let plain = format!(
        "{}\\Software\\{scratch}\\Windows Error Reporting",
        me().as_str()
    );
    let read: Vec<_> = setting
        .values
        .iter()
        .map(|(name, _)| OsRegistry.read(Root::Users, &plain, name))
        .collect();
    let removed = remove_tree(scratch);
    assert_eq!(first, Ok(()));
    assert_eq!(second, Ok(()));
    assert_eq!(read, vec![Ok(Some(1)); 4]);
    assert_eq!(removed, ERROR_SUCCESS);
}

/// Makes `HKEY_USERS\<me>\<link>` a registry link to `\REGISTRY\USER\<me>\<target>`.
fn plant_link(link: &str, target: &str) -> WIN32_ERROR {
    let to: Vec<u8> = format!("\\REGISTRY\\USER\\{}\\{target}", me().as_str())
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let path = under_me(link);
    let mut key = HKEY::default();
    // SAFETY: the path is live and NUL-terminated; `key` is a live out-pointer, closed below.
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_USERS,
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

#[test]
fn a_registry_link_is_refused_then_set_ups_repair_deletes_the_link_not_its_target() {
    let scratch = scratch();
    let target = format!("Software\\{scratch}\\target");
    let link = format!("Software\\{scratch}\\Windows Error Reporting");
    let plain_target = format!("{}\\{target}", me().as_str());
    let made = apply(&mut OsRegistry, &me(), &setting_at(scratch, "target"));
    let planted = plant_link(&link, &target);
    let setting = setting_at(scratch, "Windows Error Reporting");
    let refused = apply(&mut OsRegistry, &me(), &setting);
    let repaired = repair_one(&mut OsRegistry, &me(), &setting);
    let again = apply(&mut OsRegistry, &me(), &setting);
    let target_kept = OsRegistry.read(Root::Users, &plain_target, "Disabled");
    let removed = remove_tree(scratch);
    assert_eq!(made, Ok(()));
    assert_eq!(planted, ERROR_SUCCESS);
    assert_eq!(
        refused,
        Err(WerError::Link {
            part: "Windows Error Reporting"
        })
    );
    assert_eq!(repaired, Ok(true));
    assert_eq!(again, Ok(()));
    assert_eq!(target_kept, Ok(Some(1)));
    assert_eq!(removed, ERROR_SUCCESS);
}
