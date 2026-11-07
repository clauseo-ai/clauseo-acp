//! The SIDs a token holds, as strings: its user, and the group with a given account name.

#![expect(unsafe_code, reason = "FFI to the token and SID lookups")]

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, PSID, SID_NAME_USE, TOKEN_GROUPS,
    TOKEN_INFORMATION_CLASS, TOKEN_USER, TokenGroups, TokenUser,
};
use windows::core::{PCWSTR, PWSTR};

use super::tokens::Token;

/// `sid` as a string.
fn text(sid: PSID) -> String {
    let mut out = PWSTR::null();
    // SAFETY: `sid` is valid; `out` is LocalAlloc'd and freed below.
    unsafe {
        if ConvertSidToStringSidW(sid, &raw mut out).is_err() {
            return String::new();
        }
        let owned = out.to_string().unwrap_or_default();
        LocalFree(Some(HLOCAL(out.0.cast())));
        owned
    }
}

/// The account name of `sid`.
fn name(sid: PSID) -> String {
    let (mut name, mut domain) = (vec![0u16; 256], vec![0u16; 256]);
    let (mut name_len, mut domain_len) = (256u32, 256u32);
    let mut kind = SID_NAME_USE::default();
    // SAFETY: the buffers and their lengths match.
    let found = unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            sid,
            Some(PWSTR(name.as_mut_ptr())),
            &raw mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_len,
            &raw mut kind,
        )
    };
    if found.is_err() {
        return String::new();
    }
    String::from_utf16_lossy(name.get(..name_len as usize).unwrap_or_default())
}

/// `token`'s information of class `class`, in an aligned buffer.
fn info(token: &Token, class: TOKEN_INFORMATION_CLASS) -> Vec<u64> {
    let mut needed = 0u32;
    // SAFETY: a size query.
    drop(unsafe { GetTokenInformation(token.0, class, None, 0, &raw mut needed) });
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    let len = u32::try_from(buffer.len() * 8).unwrap_or_default();
    // SAFETY: the buffer holds `len` aligned bytes.
    drop(unsafe {
        GetTokenInformation(
            token.0,
            class,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut needed,
        )
    });
    buffer
}

/// `token`'s user SID as a string.
pub(crate) fn user(token: &Token) -> Result<String, String> {
    let buffer = info(token, TokenUser);
    // SAFETY: the buffer holds the TOKEN_USER GetTokenInformation wrote.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let found = text(sid);
    if found.is_empty() {
        Err("no user SID".to_owned())
    } else {
        Ok(found)
    }
}

/// The SID string of the group in `token` named `group`.
pub(crate) fn group(token: &Token, group: &str) -> Result<String, String> {
    let buffer = info(token, TokenGroups);
    // SAFETY: the buffer holds the TOKEN_GROUPS GetTokenInformation wrote, GroupCount entries.
    let all = unsafe {
        let groups = &*buffer.as_ptr().cast::<TOKEN_GROUPS>();
        std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
    };
    all.iter()
        .find(|g| name(g.Sid).eq_ignore_ascii_case(group))
        .map(|g| text(g.Sid))
        .ok_or_else(|| format!("the token doesn't hold the group {group}"))
}
