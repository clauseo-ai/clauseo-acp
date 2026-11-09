//! The per-user class removal on a real machine (N3): an in-process server planted for the Task
//! Scheduler's class, in both registry views, is gone, with no COM call made. The keys are planted
//! under a scratch root of this user's own `HKEY_CURRENT_USER`, never its real classes.
#![expect(unsafe_code, reason = "FFI to the registry in tests")]

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegSetValueExW,
};
use windows::core::PCWSTR;

use super::super::{Kind, remove_all};
use super::*;
use crate::windows::token::os::own_token;

/// The Task Scheduler's class: a class a runner of the pool user's could make in-process.
const TASK_SCHEDULER: &str = "{0F87369F-A4E5-4CFC-BD3E-73E6154572DD}";

/// Makes `HKEY_CURRENT_USER\<path>` with `dll` as its default value.
fn plant(path: &str, dll: &str) {
    let wide_path = wide(path);
    let mut key = HKEY::default();
    // SAFETY: the path is NUL-terminated; `key` is a live out-pointer, closed below.
    let code = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide_path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &raw mut key,
            None,
        )
    };
    assert_eq!(code, ERROR_SUCCESS, "{path}");
    let value: Vec<u8> = wide(dll)
        .iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    // SAFETY: the key is open with KEY_WRITE; `value` is a NUL-terminated UTF-16 string.
    let set = unsafe { RegSetValueExW(key, PCWSTR::null(), None, REG_SZ, Some(&value)) };
    // SAFETY: opened above, closed once.
    let _closed = unsafe { RegCloseKey(key) };
    assert_eq!(set, ERROR_SUCCESS);
}

fn exists(path: &str) -> bool {
    let wide_path = wide(path);
    open_key(PCWSTR(wide_path.as_ptr()), KEY_QUERY_VALUE)
        .unwrap()
        .is_some()
}

#[test]
fn a_planted_task_scheduler_server_is_removed() {
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap();
    let scratch = format!(
        "Software\\ClauseoTest-{}",
        crate::windows::pipe::hex(&random)
    );
    let classes = format!("{scratch}\\Classes");
    let planted = [
        format!("{classes}\\CLSID\\{TASK_SCHEDULER}\\InprocServer32"),
        format!("{classes}\\Wow6432Node\\CLSID\\{TASK_SCHEDULER}\\InprocServer32"),
        format!("{classes}\\Interface\\{TASK_SCHEDULER}"),
    ];
    for path in &planted {
        plant(path, r"C:\Users\Public\evil.dll");
    }
    let token = own_token().unwrap();
    let mut calls = OsLeftovers::new(&token);
    calls.classes.clone_from(&classes);
    let mut lines = Vec::new();
    let clean = remove_all(
        &mut calls,
        &[Kind::Classes],
        "test",
        &mut |_kind, detail| {
            lines.push(detail.text().to_owned());
        },
    );
    let left: Vec<&String> = planted.iter().filter(|path| exists(path)).collect();
    drop(calls);
    // The scratch root goes whatever the result.
    drop(delete_tree(&wide(&scratch)));
    assert!(clean, "{lines:?}");
    assert!(left.is_empty(), "left: {left:?}");
    assert_eq!(
        lines,
        ["clauseo-sandbox-run: cleanup (test): removed 3 per-user COM class keys"]
    );
    assert!(!exists(&format!("{classes}\\CLSID")));
}
