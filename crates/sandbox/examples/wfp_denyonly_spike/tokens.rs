//! The spike's accounts and tokens: a throwaway user and group made with `net`, their SIDs, the
//! user's logon token and the tokens derived from it, the admin's own, `SYSTEM`'s, and a child
//! started under each with `CreateProcessWithTokenW`.

#![expect(unsafe_code, reason = "FFI to the logon, token and process calls")]

use std::ffi::c_void;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LUID, LocalFree, WAIT_OBJECT_0};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    ACL, AdjustTokenPrivileges, CREATE_RESTRICTED_TOKEN_FLAGS, CreateRestrictedToken,
    DuplicateTokenEx, GetSecurityDescriptorDacl, LOGON32_LOGON, LOGON32_LOGON_BATCH,
    LOGON32_LOGON_INTERACTIVE, LOGON32_LOGON_NETWORK_CLEARTEXT, LOGON32_PROVIDER_DEFAULT,
    LUID_AND_ATTRIBUTES, LogonUserW, LookupPrivilegeValueW, PSECURITY_DESCRIPTOR, PSID,
    SE_PRIVILEGE_ENABLED, SECURITY_ATTRIBUTES, SID_AND_ATTRIBUTES, SecurityImpersonation,
    SetTokenInformation, TOKEN_ALL_ACCESS, TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE,
    TOKEN_MANDATORY_LABEL, TOKEN_PRIVILEGES, TOKEN_QUERY, TokenDefaultDacl, TokenIntegrityLevel,
    TokenPrimary,
};
use windows::Win32::System::ProcessStatus::EnumProcesses;
use windows::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_PROCESS_LOGON_FLAGS, CreateProcessWithTokenW, GetCurrentProcess,
    GetExitCodeProcess, OpenProcess, OpenProcessToken, PROCESS_INFORMATION, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW, STARTUPINFOW,
    WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// Runs `net <args>`; `Err` with its output when it fails.
#[expect(
    clippy::disallowed_methods,
    reason = "the throwaway spike makes and deletes its own account with net.exe"
)]
pub(crate) fn net(args: &[&str]) -> Result<(), String> {
    let out = std::process::Command::new("net")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// Runs `netsh <args>`, ignoring the result (a rule already gone is fine).
#[expect(
    clippy::disallowed_methods,
    reason = "the throwaway spike deletes the firewall rules it measured without"
)]
pub(crate) fn netsh(args: &[&str]) {
    drop(std::process::Command::new("netsh").args(args).output());
}

/// A SID string as a `PSID`, freed on drop.
pub(crate) struct OwnedSid(pub(crate) PSID);

impl OwnedSid {
    pub(crate) fn parse(text: &str) -> Result<OwnedSid, String> {
        let mut sid = PSID::default();
        // SAFETY: a live string and out-pointer; freed by Drop.
        unsafe { ConvertStringSidToSidW(&HSTRING::from(text), &raw mut sid) }
            .map_err(|e| format!("ConvertStringSidToSidW {text}: {e}"))?;
        Ok(OwnedSid(sid))
    }
}

impl Drop for OwnedSid {
    fn drop(&mut self) {
        // SAFETY: from ConvertStringSidToSidW, freed once.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// A token handle, closed on drop.
pub(crate) struct Token(pub(crate) HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: an open handle, closed once.
        drop(unsafe { CloseHandle(self.0) });
    }
}

/// A primary copy of `token`.
fn primary(token: HANDLE) -> Result<Token, String> {
    let mut copy = HANDLE::default();
    // SAFETY: `token` is open; `copy` is a live out-pointer.
    unsafe {
        DuplicateTokenEx(
            token,
            TOKEN_ALL_ACCESS,
            None,
            SecurityImpersonation,
            TokenPrimary,
            &raw mut copy,
        )
    }
    .map_err(|e| format!("DuplicateTokenEx: {e}"))?;
    Ok(Token(copy))
}

/// Logs `name` on with `password`; which logon type worked, and a primary token.
pub(crate) fn logon(name: &str, password: &str) -> Result<(&'static str, Token), String> {
    let kinds: [(&str, LOGON32_LOGON); 3] = [
        ("interactive", LOGON32_LOGON_INTERACTIVE),
        ("batch", LOGON32_LOGON_BATCH),
        ("network-cleartext", LOGON32_LOGON_NETWORK_CLEARTEXT),
    ];
    let mut why = Vec::new();
    for (label, kind) in kinds {
        let mut token = HANDLE::default();
        // SAFETY: live strings and out-pointer; the token is closed by `Token`.
        let done = unsafe {
            LogonUserW(
                &HSTRING::from(name),
                &HSTRING::from("."),
                &HSTRING::from(password),
                kind,
                LOGON32_PROVIDER_DEFAULT,
                &raw mut token,
            )
        };
        match done {
            Ok(()) => {
                let raw = Token(token);
                return primary(raw.0).map(|t| (label, t));
            }
            Err(e) => why.push(format!("{label}: {e}")),
        }
    }
    Err(why.join("; "))
}

/// This process's own token, as a primary copy.
pub(crate) fn own() -> Result<Token, String> {
    let mut token = HANDLE::default();
    // SAFETY: the pseudo-handle needs no close; `token` is closed by `Token`.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &raw mut token) }
        .map_err(|e| format!("OpenProcessToken: {e}"))?;
    let own = Token(token);
    primary(own.0)
}

/// How to derive a token.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Derive {
    /// The token as it is.
    Plain,
    /// The user SID deny-only.
    DenyUser,
    /// The user SID and the group deny-only.
    DenyUserGroup,
    /// Restricting {user, RESTRICTED, Users, Everyone} (design B).
    BToken,
    /// Restricting {user, RESTRICTED, Users}.
    NoEveryone,
    /// Restricting {user, Users}.
    UserAndUsers,
    /// Low integrity.
    Low,
}

/// `token` derived as `how`; `user` and `group` are the SID strings it holds.
pub(crate) fn derive(
    token: &Token,
    how: Derive,
    user: &str,
    group: Option<&str>,
) -> Result<Token, String> {
    let entry = |sid: PSID| SID_AND_ATTRIBUTES {
        Sid: sid,
        Attributes: 0,
    };
    let named = |list: &[&str]| -> Result<Vec<OwnedSid>, String> {
        list.iter().map(|s| OwnedSid::parse(s)).collect()
    };
    let (disable, restrict): (Vec<OwnedSid>, Vec<OwnedSid>) = match how {
        Derive::Plain => return primary(token.0),
        Derive::Low => return low(token),
        Derive::DenyUser => (named(&[user])?, vec![]),
        Derive::DenyUserGroup => {
            let mut both = vec![user];
            both.extend(group);
            (named(&both)?, vec![])
        }
        Derive::BToken => (
            vec![],
            named(&[user, "S-1-5-12", "S-1-5-32-545", "S-1-1-0"])?,
        ),
        Derive::NoEveryone => (vec![], named(&[user, "S-1-5-12", "S-1-5-32-545"])?),
        Derive::UserAndUsers => (vec![], named(&[user, "S-1-5-32-545"])?),
    };
    let disable: Vec<SID_AND_ATTRIBUTES> = disable.iter().map(|s| entry(s.0)).collect();
    let restrict: Vec<SID_AND_ATTRIBUTES> = restrict.iter().map(|s| entry(s.0)).collect();
    let mut out = HANDLE::default();
    // SAFETY: the token is open; the SID arrays point at SIDs alive for the call.
    unsafe {
        CreateRestrictedToken(
            token.0,
            CREATE_RESTRICTED_TOKEN_FLAGS(0),
            (!disable.is_empty()).then_some(disable.as_slice()),
            None,
            (!restrict.is_empty()).then_some(restrict.as_slice()),
            &raw mut out,
        )
    }
    .map_err(|e| format!("CreateRestrictedToken: {e}"))?;
    let out = Token(out);
    open_default_dacl(&out)?;
    Ok(out)
}

/// Gives `token` the default DACL {`Everyone`, `RESTRICTED`, `SYSTEM`}: a derived token's own
/// default DACL names its user (deny-only now) or Administrators (not a restricting SID), so its
/// process couldn't open itself and hung at start. An agent deriving a token can do the same.
fn open_default_dacl(token: &Token) -> Result<(), String> {
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: a live string and out-pointer; freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from("D:(A;;GA;;;WD)(A;;GA;;;RC)(A;;GA;;;SY)"),
            SDDL_REVISION_1,
            &raw mut sd,
            None,
        )
    }
    .map_err(|e| format!("the default DACL: {e}"))?;
    let (mut present, mut defaulted) = (windows::core::BOOL(0), windows::core::BOOL(0));
    let mut acl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `sd` is valid; the out-pointers are live.
    let read = unsafe {
        GetSecurityDescriptorDacl(sd, &raw mut present, &raw mut acl, &raw mut defaulted)
    };
    let dacl = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
    let size = u32::try_from(size_of::<TOKEN_DEFAULT_DACL>()).unwrap_or_default();
    // SAFETY: `dacl` points into `sd`, alive until freed below.
    let set = read.and_then(|()| unsafe {
        SetTokenInformation(
            token.0,
            TokenDefaultDacl,
            (&raw const dacl).cast::<c_void>(),
            size,
        )
    });
    // SAFETY: from the conversion, freed once.
    unsafe { LocalFree(Some(HLOCAL(sd.0))) };
    set.map_err(|e| format!("SetTokenInformation (default DACL): {e}"))
}

/// A Low-integrity copy of `token`.
fn low(token: &Token) -> Result<Token, String> {
    let copy = primary(token.0)?;
    let low = OwnedSid::parse("S-1-16-4096")?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: low.0,
            Attributes: u32::from_ne_bytes(SE_GROUP_INTEGRITY.to_ne_bytes()),
        },
    };
    let size = u32::try_from(size_of::<TOKEN_MANDATORY_LABEL>()).unwrap_or_default();
    // SAFETY: `label` and its SID live through the call.
    unsafe {
        SetTokenInformation(
            copy.0,
            TokenIntegrityLevel,
            (&raw const label).cast::<c_void>(),
            size,
        )
    }
    .map_err(|e| format!("SetTokenInformation: {e}"))?;
    Ok(copy)
}

/// `SYSTEM`'s token, copied from `winlogon.exe` with `SeDebugPrivilege` enabled.
pub(crate) fn system() -> Result<Token, String> {
    let mut luid = LUID::default();
    // SAFETY: live strings and out-pointer.
    unsafe {
        LookupPrivilegeValueW(
            PCWSTR::null(),
            &HSTRING::from("SeDebugPrivilege"),
            &raw mut luid,
        )
    }
    .map_err(|e| format!("LookupPrivilegeValueW: {e}"))?;
    let mut process_token = HANDLE::default();
    // SAFETY: as in `own`.
    unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ALL_ACCESS,
            &raw mut process_token,
        )
    }
    .map_err(|e| format!("OpenProcessToken: {e}"))?;
    let process_token = Token(process_token);
    let privileges = TOKEN_PRIVILEGES {
        PrivilegeCount: 1,
        Privileges: [LUID_AND_ATTRIBUTES {
            Luid: luid,
            Attributes: SE_PRIVILEGE_ENABLED,
        }],
    };
    // SAFETY: the token is open; `privileges` lives through the call.
    unsafe {
        AdjustTokenPrivileges(
            process_token.0,
            false,
            Some(&raw const privileges),
            0,
            None,
            None,
        )
    }
    .map_err(|e| format!("AdjustTokenPrivileges: {e}"))?;
    let mut pids = vec![0u32; 4096];
    let mut needed = 0u32;
    let size = u32::try_from(pids.len() * 4).unwrap_or_default();
    // SAFETY: the buffer holds `size` bytes.
    unsafe { EnumProcesses(pids.as_mut_ptr(), size, &raw mut needed) }
        .map_err(|e| format!("EnumProcesses: {e}"))?;
    for &pid in pids.iter().take(needed as usize / 4) {
        // SAFETY: a query-only open; the handle is closed below.
        let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
        else {
            continue;
        };
        let mut name = vec![0u16; 1024];
        let mut len = 1024u32;
        // SAFETY: the buffer and its length match.
        let named = unsafe {
            QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(name.as_mut_ptr()),
                &raw mut len,
            )
        };
        let path = String::from_utf16_lossy(name.get(..len as usize).unwrap_or_default());
        let mut token = HANDLE::default();
        let found = named.is_ok()
            && path.to_ascii_lowercase().ends_with("\\winlogon.exe")
            // SAFETY: the process handle is open; `token` is closed by `Token`.
            && unsafe {
                OpenProcessToken(process, TOKEN_DUPLICATE | TOKEN_QUERY, &raw mut token)
            }
            .is_ok();
        // SAFETY: opened above, closed once.
        drop(unsafe { CloseHandle(process) });
        if found {
            let token = Token(token);
            return primary(token.0);
        }
    }
    Err("no winlogon.exe token".to_owned())
}

#[link(name = "user32")]
unsafe extern "system" {
    fn CreateWindowStationW(
        name: PCWSTR,
        flags: u32,
        access: u32,
        sa: *const SECURITY_ATTRIBUTES,
    ) -> HANDLE;
    fn GetProcessWindowStation() -> HANDLE;
    fn SetProcessWindowStation(station: HANDLE) -> i32;
    fn CreateDesktopW(
        name: PCWSTR,
        device: PCWSTR,
        mode: *const c_void,
        flags: u32,
        access: u32,
        sa: *const SECURITY_ATTRIBUTES,
    ) -> HANDLE;
}

/// A window station and desktop every probe token may use, kept open for the run: the
/// runner's own (`Service-0x0-3e7$`) grants a throwaway user and a restricted token nothing,
/// so their probes hung at start. Its name for `STARTUPINFOW::lpDesktop`.
pub(crate) struct Desktop {
    pub(crate) name: String,
    _station: HANDLE,
    _handle: HANDLE,
}

/// Makes a [`Desktop`] named for `tag`: `Everyone`, `RESTRICTED`, `Users`, both application
/// package groups, `SYSTEM` and `Administrators` get full access, with a Low label so a
/// Low-integrity probe may use it too.
pub(crate) fn desktop(tag: &str) -> Result<Desktop, String> {
    let sddl = "D:(A;;GA;;;WD)(A;;GA;;;RC)(A;;GA;;;BU)(A;;GA;;;AC)(A;;GA;;;S-1-15-2-2)\
                (A;;GA;;;SY)(A;;GA;;;BA)S:(ML;;NW;;;LW)";
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: a live string and out-pointer; the descriptor is freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl),
            SDDL_REVISION_1,
            &raw mut sd,
            None,
        )
    }
    .map_err(|e| format!("the desktop's descriptor: {e}"))?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or_default(),
        lpSecurityDescriptor: sd.0,
        bInheritHandle: false.into(),
    };
    let station_name = format!("S15Spike{tag}");
    let full = 0x000F_037F; // WINSTA_ALL_ACCESS
    // SAFETY: a live name and attributes; the station stays open in `Desktop`.
    let station = unsafe {
        CreateWindowStationW(
            PCWSTR(HSTRING::from(station_name.as_str()).as_ptr()),
            0,
            full,
            &raw const sa,
        )
    };
    let made = if station.is_invalid() {
        Err(format!(
            "CreateWindowStationW: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        // SAFETY: switch to the new station to make its desktop, then back.
        unsafe {
            let old = GetProcessWindowStation();
            SetProcessWindowStation(station);
            let desktop = CreateDesktopW(
                PCWSTR(HSTRING::from("Default").as_ptr()),
                PCWSTR::null(),
                std::ptr::null(),
                0,
                0x000F_01FF, // GENERIC_ALL for desktops (DESKTOP_* and standard rights)
                &raw const sa,
            );
            let error = std::io::Error::last_os_error();
            SetProcessWindowStation(old);
            if desktop.is_invalid() {
                Err(format!("CreateDesktopW: {error}"))
            } else {
                Ok(Desktop {
                    name: format!("{station_name}\\Default"),
                    _station: station,
                    _handle: desktop,
                })
            }
        }
    };
    // SAFETY: from the conversion, freed once.
    unsafe { LocalFree(Some(HLOCAL(sd.0))) };
    made
}

/// Runs `exe probe <step>` under `token` (`CreateProcessWithTokenW`); the probe's exit code.
pub(crate) fn run_as(
    token: &Token,
    exe: &Path,
    step: &str,
    desktop: &Desktop,
    log: &str,
) -> Result<u32, String> {
    let line = format!(
        "\"{}\" \"probe\" \"--log\" \"{log}\" \"{step}\"",
        exe.display()
    );
    let mut desktop_name: Vec<u16> = desktop
        .name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut line: Vec<u16> = line.encode_utf16().chain(std::iter::once(0)).collect();
    let folder = exe
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or_default(),
        lpDesktop: PWSTR(desktop_name.as_mut_ptr()),
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: the token is open; the line is writable; `startup` and `info` are live.
    unsafe {
        // LOGON_WITH_PROFILE: the user's profile and HKCU loaded, as a real launch has them;
        // without them the pool user's child couldn't register an AppContainer profile (run
        // 42356413018, row 11 not run).
        CreateProcessWithTokenW(
            token.0,
            CREATE_PROCESS_LOGON_FLAGS(1),
            &HSTRING::from(exe.as_os_str()),
            Some(PWSTR(line.as_mut_ptr())),
            CREATE_NO_WINDOW,
            None,
            &HSTRING::from(folder.as_str()),
            &raw const startup,
            &raw mut info,
        )
    }
    .map_err(|e| format!("CreateProcessWithTokenW: {e}"))?;
    let mut code = 0u32;
    // SAFETY: both handles came from the call and are closed once, here.
    unsafe {
        let waited = WaitForSingleObject(info.hProcess, 60_000);
        let got = GetExitCodeProcess(info.hProcess, &raw mut code);
        drop(CloseHandle(info.hThread));
        drop(CloseHandle(info.hProcess));
        if waited != WAIT_OBJECT_0 {
            return Err("the probe didn't finish in 60 s".to_owned());
        }
        got.map_err(|e| format!("GetExitCodeProcess: {e}"))?;
    }
    Ok(code)
}
