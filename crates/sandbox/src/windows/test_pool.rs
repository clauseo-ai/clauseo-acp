//! Tests' pool users on a real machine: a temporary local account, its logon, the Low restricted
//! token the agent would get from it, impersonation of a token on the calling thread, and opens
//! that report the Win32 error. For this crate's Windows tests only; the account is made only by
//! `elevated_` tests.
#![expect(
    unsafe_code,
    reason = "FFI to LogonUserW, CreateRestrictedToken and impersonation in tests"
)]

use std::fmt::Write as _;
use std::io::Write as _;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSidToSidW};
use windows::Win32::Security::{
    ACL, CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, GetLengthSid, ImpersonateLoggedOnUser,
    LOGON32_LOGON_INTERACTIVE, LOGON32_PROVIDER_DEFAULT, LogonUserW, PSID, RevertToSelf,
    SID_AND_ATTRIBUTES, SetTokenInformation, TOKEN_MANDATORY_LABEL, TokenIntegrityLevel,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, OPEN_EXISTING,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use super::acl_bytes::{Acl, flags, kind, mask, sid, sid_text};
use super::sids::Sid;

/// `SE_GROUP_INTEGRITY`.
const SE_GROUP_INTEGRITY: u32 = 0x20;
/// The test accounts' prefix: `ClauseoSbTC` and six hex digits.
const PREFIX: &str = "ClauseoSbTC";

/// `bytes` random bytes as lowercase hex.
pub(crate) fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).unwrap();
    buffer.iter().fold(String::new(), |mut text, byte| {
        write!(text, "{byte:02x}").unwrap();
        text
    })
}

/// Runs PowerShell's `script`, `input` on its standard input; its trimmed standard output.
/// Panics when it fails.
fn powershell(script: &str, input: &str) -> String {
    run_powershell(script, input).unwrap()
}

/// [`powershell`], its failure as its standard error.
#[expect(
    clippy::disallowed_methods,
    reason = "the elevated tests make and delete their accounts with PowerShell"
)]
fn run_powershell(script: &str, input: &str) -> Result<String, String> {
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// A local account in `Users`, deleted on drop. It is never given a profile. The elevated tests
/// run one at a time, so a new one first deletes what a killed earlier run left.
pub(crate) struct TestUser {
    pub(crate) name: String,
    password: String,
    pub(crate) sid: Sid,
}

impl TestUser {
    /// Makes `ClauseoSbTC<6 hex>` with a random password, given on PowerShell's standard input.
    pub(crate) fn new() -> TestUser {
        powershell(
            &format!("Get-LocalUser -Name '{PREFIX}*' | Remove-LocalUser"),
            "",
        );
        let name = format!("{PREFIX}{}", random_hex(3));
        let password = format!("Aa1!{}", random_hex(12));
        let sid = powershell(
            &format!(
                "$p = [Console]::In.ReadLine() | ConvertTo-SecureString -AsPlainText -Force; \
                 New-LocalUser -Name '{name}' -Password $p -AccountNeverExpires | Out-Null; \
                 Add-LocalGroupMember -SID 'S-1-5-32-545' -Member '{name}'; \
                 (Get-LocalUser -Name '{name}').SID.Value"
            ),
            &format!("{password}\n"),
        );
        TestUser {
            name,
            password,
            sid: Sid::parse(&sid).unwrap(),
        }
    }
}

impl Drop for TestUser {
    fn drop(&mut self) {
        let script = format!("Remove-LocalUser -Name '{}'", self.name);
        if let Err(error) = run_powershell(&script, "") {
            eprintln!("{} wasn't deleted: {error}", self.name);
        }
    }
}

/// A handle (a token's, a file's), closed on drop.
pub(crate) struct Owned(pub(crate) HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle is open and closed once, here.
        drop(unsafe { CloseHandle(self.0) });
    }
}

/// A SID from `ConvertStringSidToSidW`, freed on drop.
pub(crate) struct BinarySid(pub(crate) PSID);

impl BinarySid {
    pub(crate) fn new(text: &str) -> BinarySid {
        let mut sid = PSID::default();
        // SAFETY: the string lives for the call; `sid` is a live out-pointer freed on drop.
        unsafe { ConvertStringSidToSidW(&HSTRING::from(text), &raw mut sid) }.unwrap();
        BinarySid(sid)
    }
}

impl Drop for BinarySid {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSidToSidW with LocalAlloc, freed once.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// `user` logged on (`LOGON32_LOGON_INTERACTIVE`): its own Medium primary token.
pub(crate) fn log_on(user: &TestUser) -> Owned {
    let name: Vec<u16> = user.name.encode_utf16().chain(once(0)).collect();
    let password: Vec<u16> = user.password.encode_utf16().chain(once(0)).collect();
    let mut logon = HANDLE::default();
    // SAFETY: the strings are NUL-terminated and live for the call; `logon` is a live
    // out-pointer, closed by `Owned`.
    unsafe {
        LogonUserW(
            PCWSTR(name.as_ptr()),
            w!("."),
            PCWSTR(password.as_ptr()),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &raw mut logon,
        )
    }
    .unwrap();
    Owned(logon)
}

/// `logon` (the token of `user`) restricted to {`user`, `RESTRICTED`, `Users`, `Everyone`},
/// every privilege but bypass traverse gone, at Low integrity.
pub(crate) fn low_restricted(logon: &Owned, user: &Sid) -> Owned {
    let sids: Vec<BinarySid> = [user.as_str(), "S-1-5-12", "S-1-5-32-545", "S-1-1-0"]
        .into_iter()
        .map(BinarySid::new)
        .collect();
    let restrict: Vec<SID_AND_ATTRIBUTES> = sids
        .iter()
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: sid.0,
            Attributes: 0,
        })
        .collect();
    let mut restricted = HANDLE::default();
    // SAFETY: the logon token is open; `restrict` and the SIDs it points to live for the call;
    // `restricted` is a live out-pointer, closed by `Owned`.
    unsafe {
        CreateRestrictedToken(
            logon.0,
            DISABLE_MAX_PRIVILEGE,
            None,
            None,
            Some(&restrict),
            &raw mut restricted,
        )
    }
    .unwrap();
    let restricted = Owned(restricted);
    let low = BinarySid::new("S-1-16-4096");
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: low.0,
            Attributes: SE_GROUP_INTEGRITY,
        },
    };
    // SAFETY: `low` is a valid SID that outlives the call.
    let sid_len = unsafe { GetLengthSid(low.0) };
    let size = u32::try_from(size_of::<TOKEN_MANDATORY_LABEL>()).unwrap() + sid_len;
    // SAFETY: the token is open with full access as CreateRestrictedToken gives it; `label`
    // and the SID it points to live for the call, which `size` covers.
    unsafe {
        SetTokenInformation(
            restricted.0,
            TokenIntegrityLevel,
            (&raw const label).cast(),
            size,
        )
    }
    .unwrap();
    restricted
}

/// [`low_restricted`] of `user`'s logon.
pub(crate) fn low_restricted_token(user: &TestUser) -> Owned {
    low_restricted(&log_on(user), &user.sid)
}

/// Impersonation of a token on this thread, reverted on drop.
pub(crate) struct Impersonating;

impl Impersonating {
    pub(crate) fn start(token: &Owned) -> Impersonating {
        // SAFETY: the token is open; the thread reverts on drop.
        unsafe { ImpersonateLoggedOnUser(token.0) }.unwrap();
        Impersonating
    }
}

impl Drop for Impersonating {
    fn drop(&mut self) {
        // SAFETY: reverts this thread's impersonation; a thread must never keep it.
        unsafe { RevertToSelf() }.unwrap();
    }
}

/// Opens the file `file` for `access`: `Ok` or the Win32 error code.
pub(crate) fn open(file: &Path, access: u32) -> Result<(), i32> {
    let name: Vec<u16> = file.as_os_str().encode_wide().chain(once(0)).collect();
    // SAFETY: `name` is NUL-terminated and lives for the call; the handle is closed by `Owned`.
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            access,
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|error| error.code().0 & 0xFFFF)?;
    drop(Owned(handle));
    Ok(())
}

/// The canonical text of the SID at `psid`.
pub(crate) fn text_of(psid: PSID) -> Sid {
    let mut text = PWSTR::null();
    // SAFETY: `psid` points at a valid SID; `text` is a live out-pointer, freed below.
    unsafe { ConvertSidToStringSidW(psid, &raw mut text) }.unwrap();
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let read = unsafe { text.to_string() }.unwrap();
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    Sid::parse(&read).unwrap()
}

/// Every entry of the ACL at `acl`, as (type, flags, mask, SID), in order.
///
/// # Safety
/// `acl` must point at a valid ACL that lives for the call.
pub(crate) unsafe fn acl_entries(acl: *const ACL) -> Vec<(u8, u8, u32, Sid)> {
    assert!(!acl.is_null(), "no DACL");
    // SAFETY: the caller's promise: a valid ACL, `AclSize` bytes long.
    let bytes = unsafe {
        let size = usize::from((*acl).AclSize);
        std::slice::from_raw_parts(acl.cast::<u8>(), size)
    };
    let parsed = Acl::parse(bytes).unwrap();
    parsed
        .aces()
        .iter()
        .map(|ace| {
            (
                kind(ace),
                flags(ace),
                mask(ace).unwrap(),
                sid(ace).and_then(sid_text).unwrap(),
            )
        })
        .collect()
}
