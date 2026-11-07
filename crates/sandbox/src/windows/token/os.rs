//! The (B) token made from the runner's own token (Windows): `CreateRestrictedToken` with
//! [`TokenSpec`]'s restricting and deny-only SIDs and `DISABLE_MAX_PRIVILEGE`, then its
//! integrity level and default DACL set with `SetTokenInformation`. A restricted copy of the
//! caller's own primary token is one `CreateProcessAsUserW` may assign with no privilege.
#![expect(unsafe_code, reason = "FFI to the token calls")]

use std::ffi::c_void;
use std::io;
use std::iter::once;
use std::ptr::null_mut;

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    ACL, CreateRestrictedToken, DISABLE_MAX_PRIVILEGE, GetLengthSid, GetSecurityDescriptorDacl,
    GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES, SetTokenInformation,
    TOKEN_ALL_ACCESS, TOKEN_DEFAULT_DACL, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS,
    TOKEN_MANDATORY_LABEL, TokenDefaultDacl, TokenGroups, TokenIntegrityLevel,
};
use windows::Win32::System::SystemServices::{SE_GROUP_INTEGRITY, SE_GROUP_LOGON_ID};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
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
pub(in crate::windows) struct LocalDescriptor(pub(crate) PSECURITY_DESCRIPTOR);

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

    /// Its DACL, valid while `self` lives.
    fn dacl(&self) -> io::Result<*mut ACL> {
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

/// `token`'s user.
pub(in crate::windows) fn user(token: &Handle) -> io::Result<Sid> {
    let text = user_of(token.0)?;
    Sid::parse(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// `token`'s logon SID: the group marked `SE_GROUP_LOGON_ID`. Exactly one, or an error.
pub(in crate::windows) fn logon_sid(token: &Handle) -> io::Result<Sid> {
    let buffer = information(token, TokenGroups)?;
    // SAFETY: GetTokenInformation wrote a TOKEN_GROUPS at the start of `buffer`, aligned for it
    // (`information` gives u64 words); its SIDs point into `buffer`, which outlives `groups`.
    let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
    let count = usize::try_from(groups.GroupCount).map_err(io::Error::other)?;
    // SAFETY: `Groups` is a variable-length array of `GroupCount` entries inside `buffer`.
    let entries = unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), count) };
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
    Ok(token)
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
fn sid_text(sid: PSID) -> io::Result<Sid> {
    let mut text = PWSTR::null();
    // SAFETY: `sid` points at a valid SID; `text` is a live out-pointer, freed below.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.map_err(|error| os_error(&error))?;
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let read = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    Sid::parse(&read?).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}
