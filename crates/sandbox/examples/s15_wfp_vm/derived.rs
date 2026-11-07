//! The probe's steps that need Windows calls of their own: a child under a derived token, an
//! ICMP echo, and a pause.
//!
//! - `denyonly`: the probe's own token restricted with its user SID in `SidsToDisable`
//!   (deny-only), which needs no privilege: the review's item 1. Set-up's blocks deny the pool
//!   user and group, which also catches deny-only SIDs, so the child must stay blocked.
//! - `lowbox`: an `AppContainer` child of the probe (`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`,
//!   capability internetClient), its package SID derived from `clauseo.s15.spike`. The
//!   folder holding the exe must grant `ALL APPLICATION PACKAGES` read and execute
//!   (`icacls C:\s15\bin /grant *S-1-15-2-1:(OI)(CI)RX`), or the child can't start, which the
//!   row shows as a failure.

#![expect(unsafe_code, reason = "FFI to the token, process and ICMP calls")]

use std::ffi::c_void;
use std::path::Path;
use std::ptr;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree, WAIT_OBJECT_0};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    ACL, CREATE_RESTRICTED_TOKEN_FLAGS, CreateRestrictedToken, GetSecurityDescriptorDacl,
    GetTokenInformation, ImpersonateAnonymousToken, PSECURITY_DESCRIPTOR, PSID, RevertToSelf,
    SECURITY_CAPABILITIES, SID_AND_ATTRIBUTES, SetTokenInformation, TOKEN_ADJUST_DEFAULT,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE, TOKEN_GROUPS,
    TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TOKEN_USER, TokenDefaultDacl, TokenGroups, TokenUser,
};
use windows::Win32::System::SystemServices::SE_GROUP_ENABLED;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessAsUserW, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, GetCurrentThread, GetExitCodeProcess,
    INFINITE, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST, OpenProcessToken,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, PROCESS_INFORMATION, STARTUPINFOEXW, STARTUPINFOW,
    UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::probe::Got;

#[link(name = "userenv")]
unsafe extern "system" {
    fn DeriveAppContainerSidFromAppContainerName(name: PCWSTR, sid: *mut PSID) -> i32;
    fn FreeSid(sid: PSID) -> *mut c_void;
}

#[link(name = "iphlpapi")]
unsafe extern "system" {
    fn IcmpCreateFile() -> HANDLE;
    fn IcmpCloseHandle(handle: HANDLE) -> i32;
    fn IcmpSendEcho(
        handle: HANDLE,
        destination: u32,
        data: *const c_void,
        size: u16,
        options: *const c_void,
        reply: *mut c_void,
        reply_size: u32,
        timeout: u32,
    ) -> u32;
}

/// A pause of a tenth of a second, for the accept loop.
#[expect(
    clippy::disallowed_methods,
    reason = "the VM harness's probe polls its listener; it has no clock to inject"
)]
pub(crate) fn pause_tenth() {
    std::thread::sleep(std::time::Duration::from_millis(100));
}

/// One ICMP echo to `ip`: reached when answered; a failed send counts as filtered when the
/// last error is `ERROR_ACCESS_DENIED`.
pub(crate) fn icmp(ip: &str) -> Got {
    let Ok(address) = ip.parse::<std::net::Ipv4Addr>() else {
        return Got::Failed;
    };
    // SAFETY: no arguments; the handle is closed below.
    let handle = unsafe { IcmpCreateFile() };
    if handle.is_invalid() {
        return Got::Failed;
    }
    let data = *b"s15";
    let mut reply = [0u8; 128];
    // The address in network order, as `IPAddr` is.
    let destination = u32::from_ne_bytes(address.octets());
    // SAFETY: the data and reply buffers live through the call and their sizes are theirs.
    let answers = unsafe {
        IcmpSendEcho(
            handle,
            destination,
            data.as_ptr().cast(),
            3,
            ptr::null(),
            reply.as_mut_ptr().cast(),
            128,
            4000,
        )
    };
    let error = std::io::Error::last_os_error().raw_os_error();
    // SAFETY: from IcmpCreateFile, closed once.
    unsafe { IcmpCloseHandle(handle) };
    match (answers, error) {
        (1.., _) => Got::Reached,
        (0, Some(5 | 10013)) => Got::Filtered,
        _ => Got::Failed,
    }
}

/// Runs `exe probe <inner>` as a child under the token `how` names; the child's exit code.
pub(crate) fn run(exe: &Path, how: &str, inner: &str) -> Result<u32, String> {
    let quoted = format!("\"{}\" \"probe\" \"{inner}\"", exe.display());
    let mut line: Vec<u16> = quoted.encode_utf16().chain(std::iter::once(0)).collect();
    match how {
        "denyonly" => deny_only(exe, &mut line, DenyOnly::User),
        "denyonlygroups" => deny_only(exe, &mut line, DenyOnly::Groups),
        "denyonlyall" => deny_only(exe, &mut line, DenyOnly::All),
        "lowbox" => lowbox(exe, &mut line, None),
        "lowboxdenyonly" => {
            let derived = restricted(DenyOnly::User)?;
            let ran = lowbox(exe, &mut line, Some(derived));
            // SAFETY: made by `restricted`, closed once.
            drop(unsafe { CloseHandle(derived) });
            ran
        }
        other => Err(format!("no derived token {other:?}")),
    }
}

/// Waits for the process `info` names and gives its exit code; closes its handles.
fn wait(info: &PROCESS_INFORMATION) -> Result<u32, String> {
    let mut code = 0u32;
    // SAFETY: both handles came from the CreateProcess call and are closed once, here.
    unsafe {
        let waited = WaitForSingleObject(info.hProcess, INFINITE);
        let got = GetExitCodeProcess(info.hProcess, &raw mut code);
        drop(CloseHandle(info.hThread));
        drop(CloseHandle(info.hProcess));
        if waited != WAIT_OBJECT_0 {
            return Err("the wait failed".to_owned());
        }
        got.map_err(|e| format!("GetExitCodeProcess: {e}"))?;
    }
    Ok(code)
}

/// Which of the token's SIDs a derived token makes deny-only.
#[derive(Clone, Copy)]
enum DenyOnly {
    /// Its user.
    User,
    /// Its local groups (`S-1-5-21-...`; the pool group among them).
    Groups,
    /// Both.
    All,
}

/// `token`'s information of `class`, in an aligned buffer.
fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<u64>, String> {
    let mut needed = 0u32;
    // SAFETY: a size query.
    drop(unsafe { GetTokenInformation(token, class, None, 0, &raw mut needed) });
    let mut buffer = vec![0u64; usize::try_from(needed).unwrap_or(0).div_ceil(8).max(1)];
    let len = u32::try_from(buffer.len() * 8).unwrap_or_default();
    // SAFETY: the buffer holds `len` aligned bytes.
    unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut needed,
        )
    }
    .map_err(|e| format!("GetTokenInformation: {e}"))?;
    Ok(buffer)
}

/// Whether `sid` is a local or domain account's (`S-1-5-21-...`).
fn is_account(sid: PSID) -> bool {
    let mut text = PWSTR::null();
    // SAFETY: `sid` is live; `text` is LocalAlloc'd and freed below.
    if unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.is_err() {
        return false;
    }
    // SAFETY: the conversion gave a NUL-terminated string, freed once, here.
    unsafe {
        let account = text.to_string().is_ok_and(|s| s.starts_with("S-1-5-21-"));
        LocalFree(Some(HLOCAL(text.0.cast())));
        account
    }
}

/// A primary copy of this process's token with `which` SIDs deny-only and a default DACL its
/// process can open; the caller closes it.
fn restricted(which: DenyOnly) -> Result<HANDLE, String> {
    let mut token = HANDLE::default();
    let access = TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT;
    // SAFETY: the pseudo-handle needs no close; `token` is closed below.
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut token) }
        .map_err(|e| format!("OpenProcessToken: {e}"))?;
    let made = token_info(token, TokenUser).and_then(|user| {
        let groups = token_info(token, TokenGroups)?;
        let mut disable = Vec::new();
        if matches!(which, DenyOnly::User | DenyOnly::All) {
            // SAFETY: the buffer holds the TOKEN_USER just written.
            let user = unsafe { &*user.as_ptr().cast::<TOKEN_USER>() };
            disable.push(SID_AND_ATTRIBUTES {
                Sid: user.User.Sid,
                Attributes: 0,
            });
        }
        if matches!(which, DenyOnly::Groups | DenyOnly::All) {
            // SAFETY: the buffer holds the TOKEN_GROUPS just written, GroupCount entries.
            let all = unsafe {
                let head = &*groups.as_ptr().cast::<TOKEN_GROUPS>();
                std::slice::from_raw_parts(head.Groups.as_ptr(), head.GroupCount as usize)
            };
            for group in all.iter().filter(|g| is_account(g.Sid)) {
                disable.push(SID_AND_ATTRIBUTES {
                    Sid: group.Sid,
                    Attributes: 0,
                });
            }
        }
        let mut derived = HANDLE::default();
        // SAFETY: the token is open; the SIDs live in `user` and `groups` through the call.
        unsafe {
            CreateRestrictedToken(
                token,
                CREATE_RESTRICTED_TOKEN_FLAGS(0),
                Some(&disable),
                None,
                None,
                &raw mut derived,
            )
        }
        .map_err(|e| format!("CreateRestrictedToken: {e}"))?;
        if let Err(why) = open_default_dacl(derived) {
            // SAFETY: made above, closed once.
            drop(unsafe { CloseHandle(derived) });
            return Err(why);
        }
        Ok(derived)
    });
    // SAFETY: opened above, closed once.
    drop(unsafe { CloseHandle(token) });
    made
}

/// The child under this process's token with `which` SIDs deny-only.
fn deny_only(exe: &Path, line: &mut [u16], which: DenyOnly) -> Result<u32, String> {
    let derived = restricted(which)?;
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or_default(),
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: a restricted copy of this process's own token needs no privilege; the line is
    // writable; `startup` and `info` are live.
    let started = unsafe {
        CreateProcessAsUserW(
            Some(derived),
            &HSTRING::from(exe.as_os_str()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &raw const startup,
            &raw mut info,
        )
    };
    // SAFETY: made above, closed once.
    drop(unsafe { CloseHandle(derived) });
    started.map_err(|e| format!("CreateProcessAsUserW: {e}"))?;
    wait(&info)
}

/// Connects to `to` while this thread impersonates the anonymous logon, then reverts.
pub(crate) fn anonymous(to: &str) -> Got {
    let Ok(to) = to.parse::<std::net::SocketAddr>() else {
        return Got::Failed;
    };
    // SAFETY: this thread impersonates the anonymous token until RevertToSelf below.
    if unsafe { ImpersonateAnonymousToken(GetCurrentThread()) }.is_err() {
        return Got::NotRun;
    }
    let result = std::net::TcpStream::connect_timeout(&to, std::time::Duration::from_secs(5));
    // SAFETY: ends this thread's impersonation.
    drop(unsafe { RevertToSelf() });
    match result {
        Ok(_) => Got::Reached,
        Err(error) if error.raw_os_error() == Some(10013) => Got::Filtered,
        Err(_) => Got::Failed,
    }
}

/// Gives `token` the default DACL {`Everyone`, `RESTRICTED`, `SYSTEM`}. With the user SID
/// deny-only, the token's own default DACL (its user) no longer lets the process open itself, so
/// it wouldn't start; a hijacked agent can make the same change to its own derived token.
fn open_default_dacl(token: HANDLE) -> Result<(), String> {
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
    let mut acl: *mut ACL = ptr::null_mut();
    // SAFETY: `sd` is valid; the out-pointers are live.
    let read = unsafe {
        GetSecurityDescriptorDacl(sd, &raw mut present, &raw mut acl, &raw mut defaulted)
    };
    let dacl = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
    let size = u32::try_from(size_of::<TOKEN_DEFAULT_DACL>()).unwrap_or_default();
    // SAFETY: `dacl` points into `sd`, alive until freed below.
    let set = read.and_then(|()| unsafe {
        SetTokenInformation(
            token,
            TokenDefaultDacl,
            (&raw const dacl).cast::<c_void>(),
            size,
        )
    });
    // SAFETY: from the conversion, freed once.
    unsafe { LocalFree(Some(HLOCAL(sd.0))) };
    set.map_err(|e| format!("SetTokenInformation (default DACL): {e}"))
}

/// The child as an `AppContainer` process with the internetClient capability, of `token` when
/// given (else of this process's).
fn lowbox(exe: &Path, line: &mut [u16], token: Option<HANDLE>) -> Result<u32, String> {
    let mut package = PSID::default();
    let name = HSTRING::from("clauseo.s15.spike");
    // SAFETY: a live name and out-pointer; the SID is freed with FreeSid below.
    let hr = unsafe {
        DeriveAppContainerSidFromAppContainerName(PCWSTR(name.as_ptr()), &raw mut package)
    };
    if hr != 0 {
        return Err(format!(
            "DeriveAppContainerSidFromAppContainerName: {hr:#x}"
        ));
    }
    let mut capability = PSID::default();
    // SAFETY: a live string and out-pointer; freed with LocalFree below.
    let parsed =
        unsafe { ConvertStringSidToSidW(&HSTRING::from("S-1-15-3-1"), &raw mut capability) };
    let result = parsed
        .map_err(|e| format!("ConvertStringSidToSidW: {e}"))
        .and_then(|()| start_in_container(exe, line, package, capability, token));
    // SAFETY: each SID came from its call above and is freed once, here.
    unsafe {
        FreeSid(package);
        LocalFree(Some(HLOCAL(capability.0)));
    }
    result
}

/// Starts `line` with the security capabilities of `package` and `capability`, and waits.
fn start_in_container(
    exe: &Path,
    line: &mut [u16],
    package: PSID,
    capability: PSID,
    token: Option<HANDLE>,
) -> Result<u32, String> {
    let mut capabilities = [SID_AND_ATTRIBUTES {
        Sid: capability,
        Attributes: u32::from_ne_bytes(SE_GROUP_ENABLED.to_ne_bytes()),
    }];
    let security = SECURITY_CAPABILITIES {
        AppContainerSid: package,
        Capabilities: capabilities.as_mut_ptr(),
        CapabilityCount: 1,
        Reserved: 0,
    };
    let mut size = 0usize;
    // SAFETY: a size query, as documented: no list, one attribute.
    drop(unsafe { InitializeProcThreadAttributeList(None, 1, None, &raw mut size) });
    let mut storage = vec![0u64; size.div_ceil(8).max(1)];
    let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
    // SAFETY: `storage` holds `size` bytes for the list.
    unsafe { InitializeProcThreadAttributeList(Some(list), 1, None, &raw mut size) }
        .map_err(|e| format!("InitializeProcThreadAttributeList: {e}"))?;
    // SAFETY: the list is initialised; `security` and what it points at outlive the process
    // start below.
    let updated = unsafe {
        UpdateProcThreadAttribute(
            list,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            Some((&raw const security).cast()),
            size_of::<SECURITY_CAPABILITIES>(),
            None,
            None,
        )
    };
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>()).unwrap_or_default();
    startup.lpAttributeList = list;
    let mut info = PROCESS_INFORMATION::default();
    let started = updated.and_then(|()| {
        let flags = CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT;
        let start = (&raw const startup).cast::<STARTUPINFOW>();
        let app = HSTRING::from(exe.as_os_str());
        let line = Some(PWSTR(line.as_mut_ptr()));
        // SAFETY: the line is writable; `startup` carries the live attribute list; `info` is
        // live; `token`, when given, is a primary token derived from this process's own.
        unsafe {
            match token {
                Some(token) => CreateProcessAsUserW(
                    Some(token),
                    &app,
                    line,
                    None,
                    None,
                    false,
                    flags,
                    None,
                    PCWSTR::null(),
                    start,
                    &raw mut info,
                ),
                None => CreateProcessW(
                    &app,
                    line,
                    None,
                    None,
                    false,
                    flags,
                    None,
                    PCWSTR::null(),
                    start,
                    &raw mut info,
                ),
            }
        }
    });
    // SAFETY: initialised above, deleted once, after the start.
    unsafe { DeleteProcThreadAttributeList(list) };
    started.map_err(|e| format!("CreateProcessW (AppContainer): {e}"))?;
    wait(&info)
}
