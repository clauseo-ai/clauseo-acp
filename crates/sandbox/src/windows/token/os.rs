//! The (B) token made from the runner's own token (Windows): `CreateRestrictedToken` with
//! [`TokenSpec`]'s restricting and deny-only SIDs and `DISABLE_MAX_PRIVILEGE`, then its
//! integrity level and default DACL set with `SetTokenInformation`. A restricted copy of the
//! caller's own primary token is one `CreateProcessAsUserW` may assign with no privilege.
#![expect(unsafe_code, reason = "FFI to the token calls")]

use std::ffi::c_void;
use std::io;
use std::iter::once;
use std::ptr::null_mut;

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, HANDLE, HLOCAL,
    LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, SDDL_REVISION_1, SE_KERNEL_OBJECT, SetSecurityInfo,
};
use windows::Win32::Security::{
    ACL, CreateRestrictedToken, DACL_SECURITY_INFORMATION, DISABLE_MAX_PRIVILEGE, GetLengthSid,
    GetSecurityDescriptorDacl, GetSecurityDescriptorSacl, GetTokenInformation,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES,
    SetTokenInformation, TOKEN_ALL_ACCESS, TOKEN_DEFAULT_DACL, TOKEN_GROUPS,
    TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TokenDefaultDacl, TokenGroups,
    TokenIntegrityLevel,
};
use windows::Win32::Storage::FileSystem::WRITE_DAC;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::SystemServices::{SE_GROUP_INTEGRITY, SE_GROUP_LOGON_ID};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThread, GetCurrentThreadId, OpenProcessToken,
    OpenThread, THREAD_ACCESS_RIGHTS,
};
use windows::Win32::UI::Shell::GetUserProfileDirectoryW;
use windows::core::{BOOL, PCWSTR, PWSTR};

use super::TokenSpec;
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::record::user_of;
use crate::windows::sids::Sid;

/// `SE_GROUP_LOGON_ID`, as the `u32` the attributes are.
const LOGON_ID: u32 = SE_GROUP_LOGON_ID.cast_unsigned();

/// A SID made from its text with `ConvertStringSidToSidW`, freed on drop.
pub(in crate::windows) struct LocalSid(PSID);

impl LocalSid {
    /// The binary form of `sid`.
    pub(in crate::windows) fn new(sid: &Sid) -> io::Result<LocalSid> {
        let text: Vec<u16> = sid.as_str().encode_utf16().chain(once(0)).collect();
        let mut out = PSID(null_mut());
        // SAFETY: `text` is NUL-terminated and outlives the call; `out` is a live out-pointer,
        // and what it receives is freed with LocalFree on drop.
        unsafe { ConvertStringSidToSidW(PCWSTR(text.as_ptr()), &raw mut out) }
            .map_err(|error| os_error(&error))?;
        Ok(LocalSid(out))
    }

    /// The SID's pointer, valid while `self` lives.
    pub(in crate::windows) fn as_psid(&self) -> PSID {
        self.0
    }
}

impl Drop for LocalSid {
    fn drop(&mut self) {
        // SAFETY: allocated by ConvertStringSidToSidW with LocalAlloc, freed once, here.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// A security descriptor made from SDDL, freed on drop.
pub(in crate::windows) struct LocalDescriptor(PSECURITY_DESCRIPTOR);

impl LocalDescriptor {
    /// The descriptor `sddl` describes.
    pub(in crate::windows) fn new(sddl: &str) -> io::Result<LocalDescriptor> {
        let text: Vec<u16> = sddl.encode_utf16().chain(once(0)).collect();
        let mut out = PSECURITY_DESCRIPTOR(null_mut());
        // SAFETY: `text` is NUL-terminated and outlives the call; `out` is a live out-pointer,
        // and what it receives is freed with LocalFree on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &raw mut out,
                None,
            )
        }
        .map_err(|error| os_error(&error))?;
        Ok(LocalDescriptor(out))
    }

    /// The descriptor, for a `SECURITY_ATTRIBUTES`; valid while `self` lives.
    pub(in crate::windows) fn raw(&self) -> *mut c_void {
        self.0.0
    }

    /// Its DACL, valid while `self` lives.
    pub(in crate::windows) fn dacl(&self) -> io::Result<*mut ACL> {
        let mut present = BOOL(0);
        let mut dacl: *mut ACL = null_mut();
        let mut defaulted = BOOL(0);
        // SAFETY: the descriptor is valid while `self` lives; the out-pointers are live.
        unsafe {
            GetSecurityDescriptorDacl(self.0, &raw mut present, &raw mut dacl, &raw mut defaulted)
        }
        .map_err(|error| os_error(&error))?;
        if !present.as_bool() || dacl.is_null() {
            return Err(io::Error::other("the default DACL's SDDL gave no DACL"));
        }
        Ok(dacl)
    }

    /// Its SACL (a label's), valid while `self` lives.
    pub(in crate::windows) fn sacl(&self) -> io::Result<*mut ACL> {
        let mut present = BOOL(0);
        let mut sacl: *mut ACL = null_mut();
        let mut defaulted = BOOL(0);
        // SAFETY: the descriptor is valid while `self` lives; the out-pointers are live.
        unsafe {
            GetSecurityDescriptorSacl(self.0, &raw mut present, &raw mut sacl, &raw mut defaulted)
        }
        .map_err(|error| os_error(&error))?;
        if !present.as_bool() || sacl.is_null() {
            return Err(io::Error::other("the SDDL gave no SACL"));
        }
        Ok(sacl)
    }
}

impl Drop for LocalDescriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: allocated with LocalAlloc by the SDDL call and freed once, here.
            unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }
}

/// This process's primary token, opened with every right: the base the agent's token is made
/// from, and what its user and logon SID are read from.
pub(in crate::windows) fn own_token() -> io::Result<Handle> {
    let mut token = HANDLE::default();
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `token` is a live
    // out-pointer, owned by `Handle` once written.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &raw mut token) }
        .map_err(|error| os_error(&error))?;
    Ok(Handle(token))
}

/// The profile folder of `token`'s user (`GetUserProfileDirectoryW`).
pub(in crate::windows) fn profile_folder(token: &Handle) -> io::Result<String> {
    let mut buffer = vec![0u16; 1024];
    let mut len = u32::try_from(buffer.len()).map_err(io::Error::other)?;
    // SAFETY: the token is live; `buffer` is writable for `len` units.
    unsafe { GetUserProfileDirectoryW(token.0, Some(PWSTR(buffer.as_mut_ptr())), &raw mut len) }
        .map_err(|e| os_error(&e))?;
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    String::from_utf16(buffer.get(..end).unwrap_or_default()).map_err(io::Error::other)
}

/// `token`'s user.
pub(in crate::windows) fn user(token: &Handle) -> io::Result<Sid> {
    let text = user_of(token.0)?;
    Sid::parse(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// `token`'s logon SID: the group marked `SE_GROUP_LOGON_ID`. Exactly one, or an error.
pub(in crate::windows) fn logon_sid(token: &Handle) -> io::Result<Sid> {
    let buffer = information(token, TokenGroups)?;
    let bytes = size_of_val(buffer.as_slice());
    if bytes < size_of::<TOKEN_GROUPS>() {
        return Err(io::Error::other("the token's groups are too short"));
    }
    let groups = buffer.as_ptr().cast::<TOKEN_GROUPS>();
    // SAFETY: `buffer` holds at least a TOKEN_GROUPS, aligned (u64 words), as GetTokenInformation
    // wrote it.
    let count = unsafe { (*groups).GroupCount };
    let count = usize::try_from(count).map_err(io::Error::other)?;
    let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
    count
        .checked_mul(size_of::<SID_AND_ATTRIBUTES>())
        .and_then(|len| len.checked_add(offset))
        .filter(|end| *end <= bytes)
        .ok_or_else(|| io::Error::other("the token's groups overrun their buffer"))?;
    // SAFETY: the entries start at `Groups`'s offset in `buffer` and `count` of them fit before
    // its end (checked above); the pointer comes from the whole buffer, so it may cover them all,
    // and their SIDs point into `buffer`, which outlives `entries`.
    let entries = unsafe {
        std::slice::from_raw_parts(
            (&raw const (*groups).Groups).cast::<SID_AND_ATTRIBUTES>(),
            count,
        )
    };
    let mut found = entries
        .iter()
        .filter(|group| group.Attributes & LOGON_ID == LOGON_ID)
        .map(|group| sid_text(group.Sid));
    match (found.next(), found.next()) {
        (Some(sid), None) => sid,
        _ => Err(io::Error::other(
            "the runner's token has no single logon SID",
        )),
    }
}

/// Makes the agent's token from `base` (the runner's own token) as `spec` says. The result is a
/// primary token for `CreateProcessAsUserW`.
pub(in crate::windows) fn make(base: &Handle, spec: &TokenSpec) -> io::Result<Handle> {
    let restricting: Vec<LocalSid> = spec
        .restricting
        .iter()
        .map(LocalSid::new)
        .collect::<io::Result<_>>()?;
    let deny: Vec<LocalSid> = spec
        .deny_only
        .iter()
        .map(LocalSid::new)
        .collect::<io::Result<_>>()?;
    let restricting_entries: Vec<SID_AND_ATTRIBUTES> = restricting
        .iter()
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: sid.as_psid(),
            Attributes: 0,
        })
        .collect();
    let deny_entries: Vec<SID_AND_ATTRIBUTES> = deny
        .iter()
        .map(|sid| SID_AND_ATTRIBUTES {
            Sid: sid.as_psid(),
            Attributes: 0,
        })
        .collect();
    let mut token = HANDLE::default();
    // SAFETY: `base` is a live token handle with TOKEN_DUPLICATE; the SID arrays point at SIDs
    // owned by `restricting` and `deny`, which outlive the call; `token` is a live out-pointer,
    // owned by `Handle` once written.
    unsafe {
        CreateRestrictedToken(
            base.0,
            DISABLE_MAX_PRIVILEGE,
            Some(&deny_entries),
            None,
            Some(&restricting_entries),
            &raw mut token,
        )
    }
    .map_err(|error| os_error(&error))?;
    let token = Handle(token);
    set_integrity(&token, &spec.integrity)?;
    set_default_dacl(&token, &spec.default_dacl)?;
    set_kernel_dacl(token.0, &LocalDescriptor::new(&spec.token_dacl)?)?;
    Ok(token)
}

/// Shuts the inner runner to its agent, which runs as the same pool user: its primary token, its
/// process and every thread it has get protected DACLs naming `SYSTEM` and the desktop user only
/// (`token::runner_*_sddl`, `OWNER RIGHTS` held to `READ_CONTROL` so the pool user, their owner,
/// keeps no implied `WRITE_DAC`); [`shut_later`] shuts its default DACL once the agent's token is
/// made. The outer runner set the process's and the first thread's DACL
/// already, while the inner was suspended; this runs first thing after the launch is read, before
/// the agent's token is made (whose own DACL is set explicitly, `TokenSpec::token_dacl`). The
/// handles this runner holds keep their access.
pub(in crate::windows) fn shut_self(
    token: &Handle,
    desktop_user: &Sid,
    logon: &Sid,
) -> io::Result<()> {
    use super::{runner_process_sddl, runner_thread_sddl, runner_token_sddl};

    set_kernel_dacl(
        token.0,
        &LocalDescriptor::new(&runner_token_sddl(desktop_user))?,
    )?;
    // SAFETY: the pseudo-handle of this process needs no close and carries every right.
    let own_process = unsafe { GetCurrentProcess() };
    set_kernel_dacl(
        own_process,
        &LocalDescriptor::new(&runner_process_sddl(desktop_user, logon))?,
    )?;
    shut_threads(&runner_thread_sddl(desktop_user))
}

/// The rest of [`shut_self`], once the agent's token is made and before the agent starts: this
/// runner's default DACL drops the pool user ([`super::runner_default_dacl`]), so a thread the
/// system starts in it later is shut, and every thread is shut again, any started since
/// [`shut_self`] included. It waits for the agent's token because `CreateRestrictedToken` builds
/// the new token's own descriptor from this default DACL: with `OWNER RIGHTS` held to
/// `READ_CONTROL` there, `make` failed with 5 on every launch (harness run 42358986179).
pub(in crate::windows) fn shut_later(token: &Handle, desktop_user: &Sid) -> io::Result<()> {
    set_default_dacl(token, &super::runner_default_dacl(desktop_user))?;
    shut_threads(&super::runner_thread_sddl(desktop_user))
}

/// Gives every thread of this process the protected DACL of `sddl`. The current thread goes
/// through its pseudo-handle, which carries every right whatever its DACL: the outer runner shut
/// it while the inner was suspended, so opening it by its id fails with 5 (the third harness run,
/// 42354749074, failed every launch so). Every other thread is opened by its id with
/// `WRITE_DAC`; one this process's own token is refused (5) is shut already, since the agent's
/// rights there are a subset of this token's, and is left as it is. A thread that ended since the
/// snapshot (87) needs nothing.
pub(in crate::windows) fn shut_threads(sddl: &str) -> io::Result<()> {
    let thread = LocalDescriptor::new(sddl)?;
    // SAFETY: the pseudo-handle of this thread needs no close and carries every right.
    set_kernel_dacl(unsafe { GetCurrentThread() }, &thread)?;
    // SAFETY: no arguments.
    let me = unsafe { GetCurrentThreadId() };
    for id in own_threads()?.into_iter().filter(|id| *id != me) {
        // SAFETY: a thread of this process, opened by its id with WRITE_DAC; the handle is owned
        // by `Handle` and closed on drop.
        let opened = unsafe { OpenThread(THREAD_ACCESS_RIGHTS(WRITE_DAC.0), false, id) };
        match opened {
            Ok(handle) => set_kernel_dacl(Handle(handle).0, &thread)?,
            Err(error)
                if error.code() == ERROR_INVALID_PARAMETER.to_hresult()
                    || error.code() == ERROR_ACCESS_DENIED.to_hresult() => {}
            Err(error) => return Err(os_error(&error)),
        }
    }
    Ok(())
}

/// Shuts this runner's primary token to everyone but `SYSTEM` (`token::RUNNER_TOKEN_FIRST_SDDL`),
/// the inner runner's first act, before it opens a pipe: the launch, which names the desktop user
/// [`shut_self`] then adds, hasn't arrived yet.
pub(in crate::windows) fn shut_token_first(token: &Handle) -> io::Result<()> {
    set_kernel_dacl(
        token.0,
        &LocalDescriptor::new(super::RUNNER_TOKEN_FIRST_SDDL)?,
    )
}

/// Shuts the end sweeper (`runner/sweeper.rs`) to every other process of its user, after
/// [`shut_token_first`]: its default DACL first ([`super::SWEEPER_DEFAULT_DACL`]), so a thread
/// started after is shut, then every thread it has ([`super::SWEEPER_THREAD_SDDL`]). The outer
/// shut its process and first thread while it was suspended.
pub(in crate::windows) fn shut_sweeper(token: &Handle) -> io::Result<()> {
    set_default_dacl(token, super::SWEEPER_DEFAULT_DACL)?;
    shut_threads(super::SWEEPER_THREAD_SDDL)
}

/// Widens the end sweeper's primary token for its COM step, once its sweep left no process of
/// its user: `me` gets [`super::SWEEPER_COM_TOKEN_RIGHTS`] (`token::sweeper_com_token_sddl`).
pub(in crate::windows) fn widen_sweeper_for_com(token: &Handle, me: &Sid) -> io::Result<()> {
    set_kernel_dacl(
        token.0,
        &LocalDescriptor::new(&super::sweeper_com_token_sddl(me))?,
    )
}

/// Sets the protected DACL of `sddl` on the kernel object `handle` (the outer runner's full-access
/// handles to the suspended inner runner's process and thread).
pub(in crate::windows) fn shut_object(handle: HANDLE, sddl: &str) -> io::Result<()> {
    set_kernel_dacl(handle, &LocalDescriptor::new(sddl)?)
}

/// The ids of this process's threads, from a Toolhelp snapshot.
fn own_threads() -> io::Result<Vec<u32>> {
    // SAFETY: a snapshot of every thread; the handle is owned by `Handle` and closed on drop.
    let snapshot = Handle(
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(|e| os_error(&e))?,
    );
    // SAFETY: no arguments.
    let me = unsafe { GetCurrentProcessId() };
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(size_of::<THREADENTRY32>()).map_err(io::Error::other)?,
        ..THREADENTRY32::default()
    };
    let mut ids = Vec::new();
    // SAFETY: the snapshot is live and `entry` is sized for the calls.
    let mut more = unsafe { Thread32First(snapshot.0, &raw mut entry) }.is_ok();
    while more {
        if entry.th32OwnerProcessID == me {
            ids.push(entry.th32ThreadID);
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snapshot.0, &raw mut entry) }.is_ok();
    }
    if ids.is_empty() {
        return Err(io::Error::other(
            "no thread of this process in the snapshot",
        ));
    }
    Ok(ids)
}

/// Gives the kernel object `handle` the protected DACL of `descriptor`.
fn set_kernel_dacl(handle: HANDLE, descriptor: &LocalDescriptor) -> io::Result<()> {
    let dacl = descriptor.dacl()?;
    // SAFETY: the handle is live with `WRITE_DAC`; the DACL lives in `descriptor`, which
    // outlives the call.
    let code = unsafe {
        SetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(dacl.cast_const()),
            None,
        )
    };
    if code.is_err() {
        return Err(io::Error::from_raw_os_error(
            i32::try_from(code.0).unwrap_or(i32::MAX),
        ));
    }
    Ok(())
}

/// Sets `token`'s integrity level to `level`.
fn set_integrity(token: &Handle, level: &Sid) -> io::Result<()> {
    let sid = LocalSid::new(level)?;
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid.as_psid(),
            Attributes: SE_GROUP_INTEGRITY.cast_unsigned(),
        },
    };
    // SAFETY: the SID is valid for the call's length.
    let sid_len = unsafe { GetLengthSid(sid.as_psid()) };
    let len = u32::try_from(size_of::<TOKEN_MANDATORY_LABEL>())
        .map_err(io::Error::other)?
        .saturating_add(sid_len);
    // SAFETY: `label` and the SID it points at outlive the call; the length covers both, as
    // the call expects.
    unsafe {
        SetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            (&raw const label).cast::<c_void>(),
            len,
        )
    }
    .map_err(|error| os_error(&error))
}

/// Sets `token`'s default DACL to the one `sddl` describes.
fn set_default_dacl(token: &Handle, sddl: &str) -> io::Result<()> {
    let descriptor = LocalDescriptor::new(sddl)?;
    let info = TOKEN_DEFAULT_DACL {
        DefaultDacl: descriptor.dacl()?,
    };
    // SAFETY: `info` points at the DACL inside `descriptor`, which outlives the call.
    unsafe {
        SetTokenInformation(
            token.0,
            TokenDefaultDacl,
            (&raw const info).cast::<c_void>(),
            u32::try_from(size_of::<TOKEN_DEFAULT_DACL>()).map_err(io::Error::other)?,
        )
    }
    .map_err(|error| os_error(&error))
}

/// `class` of `token`, in a buffer of u64 words so any of the structures is aligned in it.
fn information(token: &Handle, class: TOKEN_INFORMATION_CLASS) -> io::Result<Vec<u64>> {
    let mut needed = 0u32;
    // SAFETY: no buffer and a zero length ask only for the size, written to `needed`.
    match unsafe { GetTokenInformation(token.0, class, None, 0, &raw mut needed) } {
        Err(error) if error.code() == ERROR_INSUFFICIENT_BUFFER.to_hresult() => {}
        Err(error) => return Err(os_error(&error)),
        Ok(()) => return Err(io::Error::other("the token gave no size")),
    }
    let bytes = usize::try_from(needed).map_err(io::Error::other)?;
    let mut buffer = vec![0u64; bytes.div_ceil(size_of::<u64>()).max(1)];
    let len = u32::try_from(size_of_val(buffer.as_slice())).map_err(io::Error::other)?;
    // SAFETY: `buffer` is live, writable and `len` bytes long for the call.
    unsafe {
        GetTokenInformation(
            token.0,
            class,
            Some(buffer.as_mut_ptr().cast()),
            len,
            &raw mut needed,
        )
    }
    .map_err(|error| os_error(&error))?;
    Ok(buffer)
}

/// The canonical text of the SID at `sid`.
pub(in crate::windows) fn sid_text(sid: PSID) -> io::Result<Sid> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` points at a valid SID; `text` is a live out-pointer, freed below.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.map_err(|error| os_error(&error))?;
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let read = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    Sid::parse(&read?).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
