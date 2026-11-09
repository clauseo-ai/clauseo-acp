//! Windows only: the SIDs the pipe's checks compare, and the two checks themselves.
//!
//! [`Sid`] is a SID copied into memory this process owns, so it can move between threads and
//! needs no freeing. [`client_is`] is the guard's peer check: it impersonates the pipe's client
//! (at identification level, all the bridge allows), opens that thread token, and reverts at
//! once; the [`Revert`] guard reverts on every path, and a thread that cannot revert ends the
//! process rather than run another line as someone else. [`owner_of`] is the bridge's: the
//! pipe's owner SID. [`Descriptor`] is the pipe's security descriptor, built from SDDL.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 SID, token and security-descriptor APIs, for the named pipe's checks"
)]

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows::Win32::Security::{
    CopySid, EqualSid, GetLengthSid, GetTokenInformation, IsValidSid, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, RevertToSelf, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};
use windows::core::{PCWSTR, PWSTR};

use super::super::checks;

/// A SID in memory this process owns. Words, not bytes: a SID's sub-authorities are `u32`s.
#[derive(Clone, Debug)]
pub(crate) struct Sid {
    words: Box<[u32]>,
}

impl Sid {
    /// The SID a string names, after [`checks::sid_text_ok`] and Windows' own conversion;
    /// `None` for anything either refuses.
    pub(crate) fn parse(text: &str) -> Option<Sid> {
        if !checks::sid_text_ok(text) {
            return None;
        }
        let wide = wide(text);
        let mut raw = PSID::default();
        // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call, and `raw` a
        // live PSID for Windows to fill with memory it allocates.
        unsafe { ConvertStringSidToSidW(PCWSTR(wide.as_ptr()), &raw mut raw) }.ok()?;
        let sid = Sid::copy_from(raw);
        // SAFETY: `raw` was allocated by ConvertStringSidToSidW with LocalAlloc, is freed once,
        // here, and is not used after; `sid` holds its own copy.
        let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(raw.0))) };
        sid
    }

    /// A copy of the SID at `raw`, which must stay valid for the call; `None` if it is not one.
    fn copy_from(raw: PSID) -> Option<Sid> {
        // SAFETY: `raw` points to memory the caller keeps alive; IsValidSid only reads it.
        if !unsafe { IsValidSid(raw) }.as_bool() {
            return None;
        }
        // SAFETY: `raw` is a valid SID (checked just above).
        let length = unsafe { GetLengthSid(raw) };
        let mut words = vec![0u32; usize::try_from(length).ok()?.div_ceil(4)].into_boxed_slice();
        // SAFETY: `words` holds at least `length` bytes, aligned for a SID, and `raw` is valid.
        unsafe { CopySid(length, PSID(words.as_mut_ptr().cast()), raw) }.ok()?;
        Some(Sid { words })
    }

    /// The SID for calls that read it. They never write through the pointer.
    pub(crate) fn as_psid(&self) -> PSID {
        PSID(self.words.as_ptr().cast_mut().cast())
    }

    /// Whether the SID at `other` (valid for the call) is this one.
    fn equals(&self, other: PSID) -> bool {
        // SAFETY: both point to valid SIDs that outlive the call; EqualSid only reads them.
        unsafe { EqualSid(self.as_psid(), other) }.is_ok()
    }

    /// Whether `other` is the same SID.
    pub(crate) fn same_as(&self, other: &Sid) -> bool {
        self.equals(other.as_psid())
    }

    /// The canonical string, `S-1-...`, for the pipe's SDDL. Never logged.
    pub(crate) fn text(&self) -> io::Result<String> {
        let mut raw = PWSTR::null();
        // SAFETY: `self` is a valid SID and `raw` a live PWSTR for Windows to fill with memory
        // it allocates.
        unsafe { ConvertSidToStringSidW(self.as_psid(), &raw mut raw) }?;
        // SAFETY: on success `raw` is a NUL-terminated UTF-16 string, read before it is freed.
        let text = String::from_utf16(unsafe { raw.as_wide() }).map_err(io::Error::other);
        // SAFETY: `raw` was allocated by ConvertSidToStringSidW with LocalAlloc and is freed
        // once, here; `text` holds its own copy.
        let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(raw.0.cast()))) };
        text
    }
}

/// The user SID of this process's own token.
pub(crate) fn current_user() -> io::Result<Sid> {
    let mut raw = HANDLE::default();
    // SAFETY: GetCurrentProcess returns a pseudo handle needing no close; `raw` is a live HANDLE
    // to fill.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut raw) }?;
    // SAFETY: `raw` is the token handle just opened, which nothing else owns.
    let token = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    token_user(&token).ok_or_else(|| io::Error::other("cannot read this process's user"))
}

/// The user SID of the token `token`, opened with `TOKEN_QUERY`.
fn token_user(token: &OwnedHandle) -> Option<Sid> {
    let token = HANDLE(token.as_raw_handle());
    let mut needed = 0u32;
    // SAFETY: a size query: no buffer, and `needed` is a live u32. It fails with the size.
    let sized = unsafe { GetTokenInformation(token, TokenUser, None, 0, &raw mut needed) };
    if sized.is_ok() || needed == 0 {
        return None;
    }
    // u64 words: TOKEN_USER holds a pointer, so the buffer is aligned for one.
    let mut buffer = vec![0u64; usize::try_from(needed).ok()?.div_ceil(8)];
    // SAFETY: `buffer` holds at least `needed` bytes, aligned for TOKEN_USER, and outlives the
    // call; `needed` is a live u32.
    unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            needed,
            &raw mut needed,
        )
    }
    .ok()?;
    // SAFETY: on success the buffer starts with a TOKEN_USER whose SID points into the buffer,
    // which lives until the copy below is made.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    Sid::copy_from(user.User.Sid)
}

/// Reverts the calling thread to the process's own identity when dropped.
struct Revert;

impl Drop for Revert {
    fn drop(&mut self) {
        // SAFETY: takes no argument; ends this thread's impersonation.
        if unsafe { RevertToSelf() }.is_err() {
            // A thread still acting as the client must run nothing more.
            crate::browser_guard::note("cannot stop impersonating a pipe client; ending");
            std::process::abort();
        }
    }
}

/// The guard's peer check: whether the client connected on the server end `pipe` runs as
/// `expected`. Impersonates the client, opens the thread's token, reverts (always, through
/// [`Revert`]), then compares the token's user. `false` on any failure.
pub(crate) fn client_is(pipe: &OwnedHandle, expected: &Sid) -> bool {
    // SAFETY: `pipe` is the open server end of a connected pipe, which the caller holds.
    if unsafe { ImpersonateNamedPipeClient(HANDLE(pipe.as_raw_handle())) }.is_err() {
        return false;
    }
    let revert = Revert;
    let mut raw = HANDLE::default();
    // OpenAsSelf must be TRUE: the access check then runs as the guard. Measured on a Windows VM
    // across accounts (Low agent, browser-account guard): TRUE opens the token and TokenUser
    // reads the agent; FALSE fails with 1346 (bad impersonation level). The identification-level
    // copy the server gets is owned by the guard with full access, so the agent's own token DACL
    // is never consulted, and never needs widening.
    // SAFETY: GetCurrentThread is a pseudo handle needing no close; `raw` is a live HANDLE to
    // fill.
    let opened = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &raw mut raw) };
    drop(revert);
    if opened.is_err() {
        return false;
    }
    // SAFETY: `raw` is the token handle just opened, which nothing else owns.
    let token = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    token_user(&token).is_some_and(|user| user.same_as(expected))
}

/// The owner SID of the kernel object `handle` (opened with `READ_CONTROL`), or `None`.
pub(crate) fn owner_of(handle: &OwnedHandle) -> Option<Sid> {
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `handle` is open; `owner` and `descriptor` are live for Windows to fill, the
    // descriptor in memory it allocates and `owner` pointing into it.
    let status = unsafe {
        GetSecurityInfo(
            HANDLE(handle.as_raw_handle()),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            None,
            None,
            Some(&raw mut descriptor),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let sid = Sid::copy_from(owner);
    // SAFETY: `descriptor` was allocated by GetSecurityInfo with LocalAlloc and is freed once,
    // here; `owner` pointed into it and is not used after.
    let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    sid
}

/// A security descriptor built from SDDL, freed on drop.
pub(crate) struct Descriptor {
    raw: PSECURITY_DESCRIPTOR,
}

impl Descriptor {
    /// The descriptor `sddl` (NUL-terminated UTF-16) describes.
    pub(crate) fn from_sddl(sddl: &[u16]) -> io::Result<Descriptor> {
        if sddl.last() != Some(&0) {
            return Err(io::Error::other("the descriptor string is not terminated"));
        }
        let mut raw = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `sddl` is NUL-terminated (checked above) and outlives the call; `raw` is live
        // for Windows to fill with memory it allocates, freed by the drop below.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &raw mut raw,
                None,
            )
        }?;
        Ok(Descriptor { raw })
    }

    /// The descriptor, valid while `self` lives.
    pub(crate) fn as_ptr(&self) -> *mut core::ffi::c_void {
        self.raw.0
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: `raw` was allocated with LocalAlloc by the conversion and is freed once, here.
        let _: HLOCAL = unsafe { LocalFree(Some(HLOCAL(self.raw.0))) };
    }
}

/// `text` as NUL-terminated UTF-16.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
