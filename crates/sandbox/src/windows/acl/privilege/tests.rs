//! Windows CI: the guard on the test's own token. Without the privileges the enable is refused
//! naming the first one the token doesn't hold, and nothing stays enabled; with them, each is
//! enabled while the guard lives and back to its state before once it drops.
#![expect(unsafe_code, reason = "FFI to read the token's privileges back")]

use windows::Win32::Foundation::{HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW,
    SE_PRIVILEGE_ENABLED, SE_PRIVILEGE_REMOVED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    TOKEN_QUERY, TokenPrivileges,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PCWSTR};

use super::*;
use crate::windows::test_dacl::{drop_privileges, elevated};

/// Whether this process's token holds `name` enabled.
fn enabled(name: &str) -> bool {
    let mut token = HANDLE::default();
    // SAFETY: a pseudo-handle and a live out-pointer; the handle is owned by `Handle` at once.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }.unwrap();
    let token = Handle(token);
    let mut luid = LUID::default();
    let wide = HSTRING::from(name);
    // SAFETY: a NUL-terminated name that outlives the call and a live out-pointer.
    unsafe { LookupPrivilegeValueW(PCWSTR::null(), &wide, &raw mut luid) }.unwrap();
    let mut buffer = vec![0_u64; 512];
    let size = u32::try_from(buffer.len() * 8).unwrap();
    let mut needed = 0_u32;
    // SAFETY: the token is open for TOKEN_QUERY; `buffer` is writable for `size` bytes.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenPrivileges,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &raw mut needed,
        )
    }
    .unwrap();
    let written = buffer.as_ptr().cast::<TOKEN_PRIVILEGES>();
    // SAFETY: the call wrote a TOKEN_PRIVILEGES into `buffer` (aligned for it): a count, then
    // that many entries from `Privileges` on, all within the `needed` bytes it wrote.
    let entries = unsafe {
        let count = (*written).PrivilegeCount as usize;
        let first = std::ptr::addr_of!((*written).Privileges).cast::<LUID_AND_ATTRIBUTES>();
        std::slice::from_raw_parts(first, count)
    };
    entries.iter().any(|e| {
        e.Luid.LowPart == luid.LowPart
            && e.Luid.HighPart == luid.HighPart
            && e.Attributes.0 & SE_PRIVILEGE_ENABLED.0 != 0
    })
}

/// A token without them (this test's, its three privileges removed, as an unelevated token
/// lacks them) is refused, naming the first, and nothing is left enabled.
#[test]
fn a_token_without_the_privileges_is_refused_naming_one() {
    drop_privileges();
    let error = Privileges::enable(&RECLAIM).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    let shown = error.to_string();
    assert!(shown.contains("SeTakeOwnershipPrivilege"), "{shown}");
    assert!(shown.contains("administrator"), "{shown}");
    assert!(RECLAIM.iter().all(|name| !enabled(name)));
}

/// An elevated token (CI's runner) gets each while the guard lives, and its state before back
/// once it drops. Skipped on a token that doesn't hold them.
#[test]
fn the_guard_enables_each_and_restores_its_state_on_drop() {
    if !elevated() {
        eprintln!("skipped: the token isn't elevated");
        return;
    }
    let before: Vec<bool> = RECLAIM.iter().map(|name| enabled(name)).collect();
    let guard = Privileges::enable(&RECLAIM).unwrap();
    assert!(RECLAIM.iter().all(|name| enabled(name)));
    drop(guard);
    let after: Vec<bool> = RECLAIM.iter().map(|name| enabled(name)).collect();
    assert_eq!(after, before);
}

/// Removes `name` alone from this test's token (nextest runs each test in its own process).
fn remove(name: &str) {
    let mut token = HANDLE::default();
    let access = TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY;
    // SAFETY: a pseudo-handle and a live out-pointer; the handle is owned by `Handle` at once.
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut token) }.unwrap();
    let token = Handle(token);
    let mut luid = LUID::default();
    let wide = HSTRING::from(name);
    // SAFETY: a NUL-terminated name that outlives the call and a live out-pointer.
    unsafe { LookupPrivilegeValueW(PCWSTR::null(), &wide, &raw mut luid) }.unwrap();
    let removed = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_REMOVED,
        }],
    };
    // SAFETY: the token is open to adjust; `removed` lives for the call.
    unsafe { AdjustTokenPrivileges(token.0, false, Some(&raw const removed), 0, None, None) }
        .unwrap();
}

/// When only the third can't be enabled, the first two are put back as they were before the
/// guard, and the refusal names the third. Skipped on a token that doesn't hold them.
#[test]
fn a_refusal_of_the_third_puts_the_first_two_back() {
    if !elevated() {
        eprintln!("skipped: the token isn't elevated");
        return;
    }
    remove("SeBackupPrivilege");
    let before: Vec<bool> = RECLAIM.iter().map(|name| enabled(name)).collect();

    let error = Privileges::enable(&RECLAIM).unwrap_err();

    assert!(error.to_string().contains("SeBackupPrivilege"), "{error}");
    let after: Vec<bool> = RECLAIM.iter().map(|name| enabled(name)).collect();
    assert_eq!(after, before);
}
