//! The Win32 calls the VM harness makes itself, beyond the crate's API: files and folders made
//! and moved by handle-free calls (the crate's write rules are for the crate, not a VM tool),
//! DACLs and owners set from SDDL, a descriptor read back as SDDL (what `icacls` shows), a pool
//! user's logon and impersonation, the setup exe started as `UserSetup` starts it, the registry
//! value the non-owner row plants, privileges, and the cleanup's account deletes.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 calls a VM harness needs beyond the crate's API"
)]

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree,
    WAIT_OBJECT_0,
};
use windows::Win32::NetworkManagement::NetManagement::{NetLocalGroupDel, NetUserDel};
use windows::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW,
    GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows::Win32::Security::{
    ACL, AdjustTokenPrivileges, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
    GetTokenInformation, ImpersonateLoggedOnUser, LOGON32_LOGON_INTERACTIVE,
    LOGON32_PROVIDER_DEFAULT, LUID_AND_ATTRIBUTES, LogonUserW, LookupAccountSidW,
    LookupPrivilegeValueW, OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, RevertToSelf, SE_PRIVILEGE_ENABLED, SID_NAME_USE,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION, TOKEN_PRIVILEGES, TOKEN_QUERY, TOKEN_USER,
    TokenElevation, TokenUser, UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows::Win32::Storage::FileSystem::{
    CREATE_ALWAYS, CreateDirectoryW, CreateFileW, DeleteFileW, FILE_ATTRIBUTE_NORMAL,
    FILE_SHARE_MODE, MOVE_FILE_FLAGS, MoveFileExW, RemoveDirectoryW, WriteFile,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_SZ, RegDeleteKeyValueW, RegDeleteTreeW, RegSetKeyValueW,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, INFINITE, OpenProcessToken, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

/// A handle, closed on drop.
struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: the handle came from the call that made this guard and is closed once, here.
        drop(unsafe { CloseHandle(self.0) });
    }
}

/// Memory Windows allocated with `LocalAlloc`, freed on drop.
struct Local(*mut core::ffi::c_void);

impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a call that allocates with LocalAlloc; freed once here.
        unsafe { LocalFree(Some(HLOCAL(self.0))) };
    }
}

fn wide(path: &Path) -> HSTRING {
    HSTRING::from(path.as_os_str())
}

fn failed(call: &str, error: &windows::core::Error) -> String {
    format!("{call} failed: {error}")
}

/// This process's token, open for querying.
fn own_token(access: windows::Win32::Security::TOKEN_ACCESS_MASK) -> Result<Owned, String> {
    let mut token = HANDLE::default();
    // SAFETY: a pseudo-handle and a live out-pointer; the handle is owned by `Owned` at once.
    unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut token) }
        .map_err(|e| failed("OpenProcessToken", &e))?;
    Ok(Owned(token))
}

/// Whether this process's token is elevated.
pub(crate) fn elevated() -> Result<bool, String> {
    let token = own_token(TOKEN_QUERY)?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut needed = 0u32;
    let size = u32::try_from(size_of::<TOKEN_ELEVATION>()).unwrap_or(0);
    // SAFETY: the token is open for TOKEN_QUERY; `elevation` is live and `size` bytes long.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size,
            &raw mut needed,
        )
    }
    .map_err(|e| failed("GetTokenInformation", &e))?;
    Ok(elevation.TokenIsElevated != 0)
}

/// This process's token user, as SID text.
pub(crate) fn current_user_sid() -> Result<String, String> {
    let token = own_token(TOKEN_QUERY)?;
    let mut buffer = vec![0u64; 64];
    let size = u32::try_from(buffer.len() * 8).unwrap_or(0);
    let mut needed = 0u32;
    // SAFETY: the token is open for TOKEN_QUERY; `buffer` is writable for `size` bytes and
    // aligned for a TOKEN_USER.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &raw mut needed,
        )
    }
    .map_err(|e| failed("GetTokenInformation", &e))?;
    // SAFETY: the call wrote a TOKEN_USER at the start of `buffer`, which outlives this read.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    sid_text(sid)
}

/// `sid` (bytes Windows holds) as text.
fn sid_text(sid: PSID) -> Result<String, String> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` points to a valid SID; `text` is a live out-pointer, freed by `Local`.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }
        .map_err(|e| failed("ConvertSidToStringSidW", &e))?;
    let _free = Local(text.0.cast());
    // SAFETY: the call wrote a NUL-terminated string, alive until `_free` drops.
    unsafe { text.to_string() }.map_err(|e| e.to_string())
}

/// SID text as Windows' bytes, freed when the guard drops.
fn sid_of(text: &str) -> Result<(PSID, Local), String> {
    let mut sid = PSID::default();
    // SAFETY: the text is NUL-terminated and outlives the call; `sid` is a live out-pointer.
    unsafe { ConvertStringSidToSidW(&HSTRING::from(text), &raw mut sid) }
        .map_err(|e| failed("ConvertStringSidToSidW", &e))?;
    Ok((sid, Local(sid.0)))
}

/// The account name of `sid` on this machine.
pub(crate) fn account_name(sid: &str) -> Result<String, String> {
    let (psid, _free) = sid_of(sid)?;
    let mut name = [0u16; 256];
    let mut domain = [0u16; 256];
    let (mut name_len, mut domain_len) = (256u32, 256u32);
    let mut kind = SID_NAME_USE::default();
    // SAFETY: `psid` is a valid SID alive until `_free` drops; both buffers are writable for
    // the lengths given; the out-pointers are live.
    unsafe {
        LookupAccountSidW(
            PCWSTR::null(),
            psid,
            Some(PWSTR(name.as_mut_ptr())),
            &raw mut name_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &raw mut domain_len,
            &raw mut kind,
        )
    }
    .map_err(|e| failed("LookupAccountSidW", &e))?;
    let end = usize::try_from(name_len).unwrap_or(0).min(name.len());
    Ok(String::from_utf16_lossy(name.get(..end).unwrap_or(&[])))
}

/// Makes the folder `path`; one already there is fine.
pub(crate) fn make_dir(path: &Path) -> Result<(), String> {
    // SAFETY: the path is NUL-terminated and outlives the call; no security attributes.
    match unsafe { CreateDirectoryW(&wide(path), None) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_ALREADY_EXISTS.to_hresult() => Ok(()),
        Err(e) => Err(failed(&format!("CreateDirectoryW {}", path.display()), &e)),
    }
}

/// Writes `bytes` as the file `path`, replacing one there.
pub(crate) fn make_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    // SAFETY: the path is NUL-terminated and outlives the call; no security attributes or
    // template; the handle is owned by `Owned` at once.
    let handle = unsafe {
        CreateFileW(
            &wide(path),
            GENERIC_WRITE.0,
            FILE_SHARE_MODE(0),
            None,
            CREATE_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| failed(&format!("CreateFileW {}", path.display()), &e))?;
    let file = Owned(handle);
    let mut written = 0u32;
    // SAFETY: the handle is open for writing; `bytes` and `written` are live for the call.
    unsafe { WriteFile(file.0, Some(bytes), Some(&raw mut written), None) }
        .map_err(|e| failed("WriteFile", &e))
}

/// Moves `from` to `to` on the same volume (a rename: the item keeps its owner and DACL).
pub(crate) fn move_file(from: &Path, to: &Path) -> Result<(), String> {
    // SAFETY: both paths are NUL-terminated and outlive the call.
    unsafe { MoveFileExW(&wide(from), &wide(to), MOVE_FILE_FLAGS(0)) }
        .map_err(|e| failed("MoveFileExW", &e))
}

/// Deletes the file `path`.
pub(crate) fn delete_file(path: &Path) -> Result<(), String> {
    // SAFETY: the path is NUL-terminated and outlives the call.
    unsafe { DeleteFileW(&wide(path)) }.map_err(|e| failed("DeleteFileW", &e))
}

/// Removes the empty folder `path`.
pub(crate) fn remove_dir(path: &Path) -> Result<(), String> {
    // SAFETY: the path is NUL-terminated and outlives the call.
    unsafe { RemoveDirectoryW(&wide(path)) }.map_err(|e| failed("RemoveDirectoryW", &e))
}

/// Gives `path` the DACL `sddl` describes (`D:...`): protected, or merged with what its parent
/// passes down (`SetNamedSecurityInfoW` adds the inherited entries and propagates below).
pub(crate) fn set_dacl(path: &Path, sddl: &str, protected: bool) -> Result<(), String> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the text is NUL-terminated and outlives the call; `descriptor` is a live
    // out-pointer, freed by `Local`.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &HSTRING::from(sddl),
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .map_err(|e| failed("ConvertStringSecurityDescriptorToSecurityDescriptorW", &e))?;
    let _free = Local(descriptor.0);
    let mut present = windows::core::BOOL(0);
    let mut defaulted = windows::core::BOOL(0);
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: the descriptor is valid until `_free` drops; the out-pointers are live.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &raw mut present,
            &raw mut dacl,
            &raw mut defaulted,
        )
    }
    .map_err(|e| failed("GetSecurityDescriptorDacl", &e))?;
    let how = if protected {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    // SAFETY: the path is NUL-terminated; the DACL points into the descriptor, alive until
    // `_free` drops.
    let status = unsafe {
        SetNamedSecurityInfoW(
            &wide(path),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | how,
            None,
            None,
            Some(dacl.cast_const()),
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!(
            "SetNamedSecurityInfoW (DACL) failed with {}",
            status.0
        ));
    }
    Ok(())
}

/// Makes `sid` the owner of `path`, owner only (an owner other than this token's user needs
/// `SeRestorePrivilege` enabled first).
pub(crate) fn set_owner(path: &Path, sid: &str) -> Result<(), String> {
    let (psid, _free) = sid_of(sid)?;
    // SAFETY: the path is NUL-terminated; `psid` is valid until `_free` drops.
    let status = unsafe {
        SetNamedSecurityInfoW(
            &wide(path),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(psid),
            None,
            None,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!(
            "SetNamedSecurityInfoW (owner) failed with {}",
            status.0
        ));
    }
    Ok(())
}

/// `path`'s owner and DACL as SDDL (`O:<sid>D:...`): what `icacls` shows, exactly.
pub(crate) fn sddl_of(path: &Path) -> Result<String, String> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let info = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    // SAFETY: the path is NUL-terminated; `descriptor` is a live out-pointer, freed by `Local`.
    let status = unsafe {
        GetNamedSecurityInfoW(
            &wide(path),
            SE_FILE_OBJECT,
            info,
            None,
            None,
            None,
            None,
            &raw mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!("GetNamedSecurityInfoW failed with {}", status.0));
    }
    let _free = Local(descriptor.0);
    let mut text = PWSTR::null();
    // SAFETY: the descriptor is valid until `_free` drops; `text` is a live out-pointer,
    // freed by `Local`.
    unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            info,
            &raw mut text,
            None,
        )
    }
    .map_err(|e| failed("ConvertSecurityDescriptorToStringSecurityDescriptorW", &e))?;
    let _text = Local(text.0.cast());
    // SAFETY: the call wrote a NUL-terminated string, alive until `_text` drops.
    unsafe { text.to_string() }.map_err(|e| e.to_string())
}

/// Runs `f` on this thread as the local user `name`, logged on with `password` (an interactive
/// logon, as the pool user's agent runs), then makes the thread itself again.
pub(crate) fn as_user<T>(name: &str, password: &str, f: impl FnOnce() -> T) -> Result<T, String> {
    let mut token = HANDLE::default();
    // SAFETY: the strings are NUL-terminated and outlive the call; `token` is a live
    // out-pointer, owned by `Owned` at once.
    unsafe {
        LogonUserW(
            &HSTRING::from(name),
            w!("."),
            &HSTRING::from(password),
            LOGON32_LOGON_INTERACTIVE,
            LOGON32_PROVIDER_DEFAULT,
            &raw mut token,
        )
    }
    .map_err(|e| failed("LogonUserW", &e))?;
    let token = Owned(token);
    // SAFETY: the token is a primary token from LogonUserW, open for impersonation.
    unsafe { ImpersonateLoggedOnUser(token.0) }
        .map_err(|e| failed("ImpersonateLoggedOnUser", &e))?;
    let result = f();
    // SAFETY: ends this thread's impersonation; nothing else holds it.
    if unsafe { RevertToSelf() }.is_err() {
        // A thread that can't stop acting as the pool user mustn't carry on as the lawyer.
        std::process::abort();
    }
    Ok(result)
}

/// Starts `exe` with `params` as `UserSetup` does (verb `open`, so its manifest raises the
/// prompt; hidden; no error box), waits, and gives its exit code.
pub(crate) fn run_exe(exe: &Path, params: &str) -> Result<u32, String> {
    let file: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let parameters: Vec<u16> = params.encode_utf16().chain([0]).collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(u32::MAX),
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("open"),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: 0,
        ..SHELLEXECUTEINFOW::default()
    };
    // SAFETY: `info` is live and sized in `cbSize`; its strings are NUL-terminated and outlive
    // the call. The process handle it returns is owned by `Owned` at once.
    unsafe { ShellExecuteExW(&raw mut info) }.map_err(|e| failed("ShellExecuteExW", &e))?;
    if info.hProcess.is_invalid() {
        return Err("ShellExecuteExW gave no process".to_owned());
    }
    let process = Owned(info.hProcess);
    // SAFETY: the handle is live and has SYNCHRONIZE.
    if unsafe { WaitForSingleObject(process.0, INFINITE) } != WAIT_OBJECT_0 {
        return Err("waiting for the setup exe failed".to_owned());
    }
    let mut code = 0u32;
    // SAFETY: the handle is live; `code` is a live out-pointer.
    unsafe { GetExitCodeProcess(process.0, &raw mut code) }
        .map_err(|e| failed("GetExitCodeProcess", &e))?;
    Ok(code)
}

/// Enables each of `names` in this process's token (the elevated steps; the process ends after).
pub(crate) fn enable_privileges(names: &[&str]) -> Result<(), String> {
    let token = own_token(TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY)?;
    for name in names {
        let mut luid = windows::Win32::Foundation::LUID::default();
        // SAFETY: the name is NUL-terminated and outlives the call; `luid` is a live out-pointer.
        unsafe { LookupPrivilegeValueW(PCWSTR::null(), &HSTRING::from(*name), &raw mut luid) }
            .map_err(|e| failed("LookupPrivilegeValueW", &e))?;
        let wanted = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        // SAFETY: the token is open to adjust; `wanted` lives for the call.
        unsafe { AdjustTokenPrivileges(token.0, false, Some(&raw const wanted), 0, None, None) }
            .map_err(|e| failed("AdjustTokenPrivileges", &e))?;
    }
    Ok(())
}

/// Sets `HKLM\<key>\<name>` to the string `value`.
pub(crate) fn reg_set_string(key: &str, name: &str, value: &str) -> Result<(), String> {
    let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
    let size = u32::try_from(data.len() * 2).unwrap_or(0);
    // SAFETY: the key and name are NUL-terminated; `data` holds `size` bytes for the call.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(key),
            &HSTRING::from(name),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            size,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!("RegSetKeyValueW failed with {}", status.0));
    }
    Ok(())
}

/// Deletes `HKLM\<key>` and everything under it; a missing key is fine.
pub(crate) fn reg_delete_tree(key: &str) -> Result<(), String> {
    // SAFETY: the key is NUL-terminated and outlives the call.
    let status = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, &HSTRING::from(key)) };
    if status != ERROR_SUCCESS && status.0 != 2 {
        return Err(format!("RegDeleteTreeW failed with {}", status.0));
    }
    Ok(())
}

/// Deletes the value `HKLM\<key>\<name>`; a missing one is fine.
pub(crate) fn reg_delete_value(key: &str, name: &str) -> Result<(), String> {
    // SAFETY: the key and name are NUL-terminated and outlive the call.
    let status = unsafe {
        RegDeleteKeyValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(key),
            &HSTRING::from(name),
        )
    };
    if status != ERROR_SUCCESS && status.0 != 2 {
        return Err(format!("RegDeleteKeyValueW failed with {}", status.0));
    }
    Ok(())
}

/// Deletes the local account `name`; one already gone (2221) is fine.
pub(crate) fn delete_user(name: &str) -> Result<(), String> {
    // SAFETY: the name is NUL-terminated and outlives the call; no server name (this one).
    let status = unsafe { NetUserDel(PCWSTR::null(), &HSTRING::from(name)) };
    match status {
        0 | 2221 => Ok(()),
        code => Err(format!("NetUserDel failed with {code}")),
    }
}

/// Deletes the local group `name`; one already gone (2220) is fine.
pub(crate) fn delete_group(name: &str) -> Result<(), String> {
    // SAFETY: the name is NUL-terminated and outlives the call; no server name (this one).
    let status = unsafe { NetLocalGroupDel(PCWSTR::null(), &HSTRING::from(name)) };
    match status {
        0 | 2220 => Ok(()),
        code => Err(format!("NetLocalGroupDel failed with {code}")),
    }
}
