//! Starting the probe as a pool user under the (B) token, for the VM rows. The desktop side logs
//! the pool user on with its password from `secrets.bin` (`CreateProcessWithLogonW`, as PR5's
//! runner does) and starts this example again as `runner`; the runner, now the pool user,
//! restricts its own token as design (B) does (restricting {the pool user, `RESTRICTED`,
//! `Users`, `Everyone`}, its logon SID deny-only, Low integrity, a default DACL of the pool user
//! and `SYSTEM`) and starts `probe` under it. The exit code comes back up both levels.
//!
//! A stand-in for PR5's runner, enough for the network rows; it makes no private desktop, so a
//! probe that loads `user32` could fail to start (`0xC0000142`), which the rows report as such.

#![expect(unsafe_code, reason = "FFI to the logon, token and process calls")]

use std::ffi::c_void;
use std::path::Path;

use clauseo_sandbox::windows::secrets;
use clauseo_sandbox::windows::sids::Sid;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree, WAIT_OBJECT_0};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    ACL, CREATE_RESTRICTED_TOKEN_FLAGS, CreateRestrictedToken, GetSecurityDescriptorDacl,
    GetTokenInformation, LookupAccountSidW, PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES,
    SID_NAME_USE, SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY,
    TOKEN_DEFAULT_DACL, TOKEN_DUPLICATE, TOKEN_GROUPS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
    TOKEN_USER, TokenDefaultDacl, TokenGroups, TokenIntegrityLevel, TokenUser,
};
use windows::Win32::System::SystemServices::{SE_GROUP_INTEGRITY, SE_GROUP_LOGON_ID};
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessAsUserW, CreateProcessWithLogonW, GetCurrentProcess,
    GetExitCodeProcess, INFINITE, LOGON_WITH_PROFILE, OpenProcessToken, PROCESS_INFORMATION,
    STARTUPINFOW, WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, PWSTR};

/// `text` as a NUL-terminated UTF-16 buffer the calls may write into.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A command line: each argument quoted (none of ours holds a quote).
fn command_line(exe: &Path, args: &[String]) -> Vec<u16> {
    let quoted: Vec<String> = std::iter::once(exe.display().to_string())
        .chain(args.iter().cloned())
        .map(|arg| format!("\"{arg}\""))
        .collect();
    wide(&quoted.join(" "))
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

/// A SID from its string, `LocalFree`d on drop.
struct OwnedSid(PSID);

impl OwnedSid {
    fn parse(text: &str) -> Result<OwnedSid, String> {
        let mut sid = PSID::default();
        // SAFETY: a live HSTRING and out-pointer; the SID is freed by Drop.
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

/// The account name and domain of `sid`.
fn account(sid: &Sid) -> Result<(String, String), String> {
    let owned = OwnedSid::parse(sid.as_str())?;
    let (mut name, mut domain) = (vec![0u16; 256], vec![0u16; 256]);
    let (mut name_len, mut domain_len) = (256u32, 256u32);
    let mut kind = SID_NAME_USE::default();
    // SAFETY: the buffers and their lengths are live and match.
    unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            owned.0,
            Some(PWSTR(name.as_mut_ptr())),
            &raw mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_len,
            &raw mut kind,
        )
    }
    .map_err(|e| format!("LookupAccountSidW {sid}: {e}"))?;
    let text = |buffer: &[u16], len: u32| {
        String::from_utf16_lossy(buffer.get(..len as usize).unwrap_or_default())
    };
    Ok((text(&name, name_len), text(&domain, domain_len)))
}

/// Runs `exe runner <steps>` as the pool user `sid`, its password read from the owner's
/// `secrets.bin` under `data_dir`; the probe's exit code.
pub(crate) fn as_pool_user(
    exe: &Path,
    data_dir: &Path,
    sid: &Sid,
    steps: &[String],
) -> Result<u32, String> {
    let password = secrets::secret_for(data_dir, sid).map_err(|e| format!("secrets.bin: {e}"))?;
    let (name, domain) = account(sid)?;
    let mut args = vec!["runner".to_owned()];
    args.extend(steps.iter().cloned());
    let mut line = command_line(exe, &args);
    let folder = exe
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or_default(),
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: every string is a live NUL-terminated buffer; `line` is writable as the call
    // needs; `startup` and `info` are live.
    unsafe {
        CreateProcessWithLogonW(
            &HSTRING::from(name.as_str()),
            &HSTRING::from(domain.as_str()),
            &HSTRING::from(password.as_str()),
            LOGON_WITH_PROFILE,
            &HSTRING::from(exe.as_os_str()),
            Some(PWSTR(line.as_mut_ptr())),
            CREATE_NO_WINDOW,
            None,
            &HSTRING::from(folder.as_str()),
            &raw const startup,
            &raw mut info,
        )
    }
    .map_err(|e| format!("CreateProcessWithLogonW as {name}: {e}"))?;
    wait(&info)
}

/// A token's information of class `class`, in an aligned buffer.
fn token_info(token: HANDLE, class: windows::Win32::Security::TOKEN_INFORMATION_CLASS) -> Vec<u64> {
    let mut needed = 0u32;
    // SAFETY: a size query: no buffer, a live length.
    drop(unsafe { GetTokenInformation(token, class, None, 0, &raw mut needed) });
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    let len = u32::try_from(buffer.len() * 8).unwrap_or_default();
    // SAFETY: the buffer holds `len` bytes, aligned for the token structures.
    drop(unsafe {
        GetTokenInformation(
            token,
            class,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut needed,
        )
    });
    buffer
}

/// The runner: restricts this process's token as design (B) does and runs `exe probe <steps>`
/// under it; the probe's exit code.
pub(crate) fn runner(exe: &Path, steps: &[String]) -> Result<u32, String> {
    let mut token = HANDLE::default();
    let access = TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY | TOKEN_QUERY | TOKEN_ADJUST_DEFAULT;
    // SAFETY: the pseudo-handle needs no close; `token` is a live out-pointer, closed below.
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut token) }
        .map_err(|e| format!("OpenProcessToken: {e}"))?;
    let user = token_info(token, TokenUser);
    let groups = token_info(token, TokenGroups);
    // SAFETY: the buffers hold what GetTokenInformation wrote for these classes.
    let (user_sid, logon) = unsafe {
        let user = &*user.as_ptr().cast::<TOKEN_USER>();
        let groups = &*groups.as_ptr().cast::<TOKEN_GROUPS>();
        let all = std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize);
        let logon = all
            .iter()
            .find(|g| g.Attributes & u32::from_ne_bytes(SE_GROUP_LOGON_ID.to_ne_bytes()) != 0)
            .map(|g| g.Sid);
        (user.User.Sid, logon)
    };
    let well_known = ["S-1-5-12", "S-1-5-32-545", "S-1-1-0"]
        .map(OwnedSid::parse)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let entry = |sid: PSID| SID_AND_ATTRIBUTES {
        Sid: sid,
        Attributes: 0,
    };
    let mut restrict = vec![entry(user_sid)];
    restrict.extend(well_known.iter().map(|s| entry(s.0)));
    let disable: Vec<SID_AND_ATTRIBUTES> = logon.into_iter().map(entry).collect();
    let mut restricted = HANDLE::default();
    // SAFETY: the token is open; the SID arrays point at SIDs alive for the call.
    let made = unsafe {
        CreateRestrictedToken(
            token,
            CREATE_RESTRICTED_TOKEN_FLAGS(0),
            Some(&disable),
            None,
            Some(&restrict),
            &raw mut restricted,
        )
    };
    // SAFETY: opened above, closed once.
    drop(unsafe { CloseHandle(token) });
    made.map_err(|e| format!("CreateRestrictedToken: {e}"))?;
    let result = low_with_dacl(restricted).and_then(|()| probe(restricted, exe, steps));
    // SAFETY: made above, closed once.
    drop(unsafe { CloseHandle(restricted) });
    result
}

/// Lowers `token` to Low integrity and gives it the default DACL {its user, `SYSTEM`}.
fn low_with_dacl(token: HANDLE) -> Result<(), String> {
    let low = OwnedSid::parse("S-1-16-4096")?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: low.0,
            Attributes: u32::from_ne_bytes(SE_GROUP_INTEGRITY.to_ne_bytes()),
        },
    };
    let size = |n: usize| u32::try_from(n).unwrap_or_default();
    // SAFETY: `label` and the SID it names live through the call.
    unsafe {
        SetTokenInformation(
            token,
            TokenIntegrityLevel,
            (&raw const label).cast::<c_void>(),
            size(size_of::<TOKEN_MANDATORY_LABEL>()),
        )
    }
    .map_err(|e| format!("SetTokenInformation (integrity): {e}"))?;
    let user_text = current_user_text(token)?;
    let sddl = format!("D:(A;;GA;;;{user_text})(A;;GA;;;SY)");
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: a live HSTRING and out-pointer; `sd` is freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl),
            SDDL_REVISION_1,
            &raw mut sd,
            None,
        )
    }
    .map_err(|e| format!("the default DACL: {e}"))?;
    let (mut present, mut defaulted) = (windows::core::BOOL(0), windows::core::BOOL(0));
    let mut acl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `sd` is a valid descriptor; the out-pointers are live.
    let read = unsafe {
        GetSecurityDescriptorDacl(sd, &raw mut present, &raw mut acl, &raw mut defaulted)
    };
    let dacl = TOKEN_DEFAULT_DACL { DefaultDacl: acl };
    // SAFETY: `dacl` points into `sd`, alive until freed below.
    let set = read.and_then(|()| unsafe {
        SetTokenInformation(
            token,
            TokenDefaultDacl,
            (&raw const dacl).cast::<c_void>(),
            size(size_of::<TOKEN_DEFAULT_DACL>()),
        )
    });
    // SAFETY: from the conversion, freed once.
    unsafe { LocalFree(Some(HLOCAL(sd.0))) };
    set.map_err(|e| format!("SetTokenInformation (default DACL): {e}"))
}

/// The token user's SID as a string.
fn current_user_text(token: HANDLE) -> Result<String, String> {
    let user = token_info(token, TokenUser);
    let mut text = PWSTR::null();
    // SAFETY: the buffer holds a TOKEN_USER; `text` is LocalAlloc'd and freed below.
    unsafe {
        let user = &*user.as_ptr().cast::<TOKEN_USER>();
        windows::Win32::Security::Authorization::ConvertSidToStringSidW(
            user.User.Sid,
            &raw mut text,
        )
        .map_err(|e| format!("ConvertSidToStringSidW: {e}"))?;
        let owned = text.to_string().unwrap_or_default();
        LocalFree(Some(HLOCAL(text.0.cast())));
        Ok(owned)
    }
}

/// Runs `exe probe <steps>` under `token`; its exit code.
fn probe(token: HANDLE, exe: &Path, steps: &[String]) -> Result<u32, String> {
    let mut args = vec!["probe".to_owned()];
    args.extend(steps.iter().cloned());
    let mut line = command_line(exe, &args);
    let startup = STARTUPINFOW {
        cb: u32::try_from(size_of::<STARTUPINFOW>()).unwrap_or_default(),
        ..STARTUPINFOW::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: the token is a restricted copy of this process's own (no privilege needed);
    // `line` is writable; `startup` and `info` are live.
    unsafe {
        CreateProcessAsUserW(
            Some(token),
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
    }
    .map_err(|e| format!("CreateProcessAsUserW: {e}"))?;
    wait(&info)
}
