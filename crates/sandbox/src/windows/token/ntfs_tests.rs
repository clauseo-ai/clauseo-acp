//! The (B) token on a real machine: `os::make` applied to this process's own token, read back.
//! Unelevated: it runs on every Windows CI run.
#![expect(
    unsafe_code,
    reason = "FFI to GetTokenInformation and LookupPrivilegeNameW in tests"
)]

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, HANDLE};
use windows::Win32::Security::{
    GetTokenInformation, LookupPrivilegeNameW, TOKEN_DEFAULT_DACL, TOKEN_GROUPS,
    TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_PRIVILEGES, TokenDefaultDacl,
    TokenGroups, TokenIntegrityLevel, TokenPrivileges, TokenRestrictedSids,
};
use windows::Win32::System::SystemServices::{SE_GROUP_ENABLED, SE_GROUP_USE_FOR_DENY_ONLY};
use windows::core::PWSTR;

use super::{DESKTOP_USER_RIGHTS, TokenSpec, os};
use crate::windows::sids::Sid;
use crate::windows::test_pool::{acl_entries, text_of};

/// `GENERIC_ALL`: a token's default DACL is kept as set, its generic rights unmapped until an
/// object takes it.
const GENERIC_ALL: u32 = 0x1000_0000;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

/// `class` of `token`, in u64 words so any of the structures is aligned in it.
fn information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Vec<u64> {
    let mut needed = 0u32;
    // SAFETY: no buffer and a zero length ask only for the size, written to `needed`.
    let sized = unsafe { GetTokenInformation(token, class, None, 0, &raw mut needed) };
    assert_eq!(
        sized.unwrap_err().code(),
        ERROR_INSUFFICIENT_BUFFER.to_hresult()
    );
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    let len = u32::try_from(size_of_val(buffer.as_slice())).unwrap();
    // SAFETY: `buffer` is live, writable and `len` bytes long.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut needed,
        )
    }
    .unwrap();
    buffer
}

/// The groups of a `TOKEN_GROUPS` that `buffer` holds, as (SID, attributes), in order.
fn groups(buffer: &[u64]) -> Vec<(Sid, u32)> {
    // SAFETY: GetTokenInformation wrote a TOKEN_GROUPS at the start of `buffer`, aligned for
    // it; its SIDs point into `buffer`.
    let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
    // SAFETY: `Groups` is a variable-length array of `GroupCount` entries inside `buffer`.
    let entries =
        unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize) };
    entries
        .iter()
        .map(|group| (text_of(group.Sid), group.Attributes))
        .collect()
}

/// The names of `token`'s privileges, in order.
fn privileges(token: HANDLE) -> Vec<String> {
    let buffer = information(token, TokenPrivileges);
    // SAFETY: GetTokenInformation wrote a TOKEN_PRIVILEGES at the start of `buffer`.
    let held = unsafe { &*buffer.as_ptr().cast::<TOKEN_PRIVILEGES>() };
    // SAFETY: `Privileges` is a variable-length array of `PrivilegeCount` entries in `buffer`.
    let entries = unsafe {
        std::slice::from_raw_parts(held.Privileges.as_ptr(), held.PrivilegeCount as usize)
    };
    entries
        .iter()
        .map(|entry| {
            let mut name = vec![0u16; 128];
            let mut len = u32::try_from(name.len()).unwrap();
            // SAFETY: `entry.Luid` lives for the call; `name` is writable for `len` characters.
            unsafe {
                LookupPrivilegeNameW(
                    None,
                    &raw const entry.Luid,
                    Some(PWSTR(name.as_mut_ptr())),
                    &raw mut len,
                )
            }
            .unwrap();
            String::from_utf16(&name[..len as usize]).unwrap()
        })
        .collect()
}

#[test]
fn the_agent_token_holds_what_the_spec_says_when_read_back() {
    let own = os::own_token().unwrap();
    let user = os::user(&own).unwrap();
    let logon = os::logon_sid(&own).unwrap();
    let desktop = sid("S-1-5-21-1-2-3-1001");
    let spec = TokenSpec::for_pool(&user, &logon, &desktop, None).unwrap();
    let made = os::make(&own, &spec).unwrap();
    let token = made.0;

    assert_eq!(privileges(token), ["SeChangeNotifyPrivilege"]);

    let buffer = information(token, TokenIntegrityLevel);
    // SAFETY: GetTokenInformation wrote a TOKEN_MANDATORY_LABEL at the start of `buffer`.
    let label = unsafe { &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    assert_eq!(text_of(label.Label.Sid), sid("S-1-16-4096"));

    let restricted: Vec<Sid> = groups(&information(token, TokenRestrictedSids))
        .into_iter()
        .map(|(sid, _)| sid)
        .collect();
    assert_eq!(
        restricted,
        [
            user.clone(),
            sid("S-1-5-12"),
            sid("S-1-5-32-545"),
            sid("S-1-1-0")
        ]
    );

    let all = groups(&information(token, TokenGroups));
    let (_, attributes) = all
        .iter()
        .find(|(group, _)| *group == logon)
        .expect("the logon SID stays, deny-only");
    assert_ne!(attributes & SE_GROUP_USE_FOR_DENY_ONLY.cast_unsigned(), 0);
    assert_eq!(attributes & SE_GROUP_ENABLED.cast_unsigned(), 0);

    let buffer = information(token, TokenDefaultDacl);
    // SAFETY: GetTokenInformation wrote a TOKEN_DEFAULT_DACL at the start of `buffer`; its
    // DACL points into `buffer`, which lives through the read.
    let dacl = unsafe { &*buffer.as_ptr().cast::<TOKEN_DEFAULT_DACL>() }.DefaultDacl;
    // SAFETY: the DACL is valid and lives in `buffer` for the call.
    let entries = unsafe { acl_entries(dacl) };
    assert_eq!(
        entries,
        [
            (0, 0, GENERIC_ALL, user),
            (0, 0, GENERIC_ALL, sid("S-1-5-18")),
            (0, 0, DESKTOP_USER_RIGHTS, desktop),
        ]
    );
}
