//! Windows: whether the agent can change a file or folder, asked of the Authz API for the agent's
//! identity rather than by opening the path as the guard. The guard may run as another account
//! than the agent (the browser account under W2), and then a binary the agent owns, and may
//! re-permission, would open read-only "for me" and pass.
//!
//! Each identity is an Authz client context: the guard's own token always (an agent that runs the
//! guard itself is that token, so naming another account with `--agent-sid` gains it nothing),
//! plus, when the host names one, the agent's user SID with its groups as Windows computes them
//! (`AuthzInitializeContextFromSid`), inside its `AppContainer` package when one is given
//! (`AuthzSetAppContainerInformation`). For each path, `AuthzAccessCheck` asks the maximum each
//! context is granted against the path's owner, group and DACL, the owner's implied `WRITE_DAC`
//! included. The mandatory label is not read: a context from a SID carries no integrity level,
//! and leaving the label out can only grant more, never less. Any right of [`CHANGE`] granted,
//! or any call failing, says modifiable ([`Writers::can_change`]).
//!
//! The access check alone is not enough: an owner may add an `OWNER RIGHTS` entry that takes its
//! own write and permission rights away, and then the check grants it nothing, though the file
//! is still its own work. So a path whose owner is any identity's user, package or group
//! (deny-only groups aside) counts as modifiable too ([`Writers::owns`]).

#![expect(
    unsafe_code,
    reason = "FFI to the Authz and security descriptor APIs, to ask what the agent may change"
)]

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, HLOCAL, LUID, LocalFree};
use windows::Win32::Security::Authorization::{
    AUTHZ_ACCESS_CHECK_FLAGS, AUTHZ_ACCESS_REPLY, AUTHZ_ACCESS_REQUEST,
    AUTHZ_CLIENT_CONTEXT_HANDLE, AUTHZ_CONTEXT_INFORMATION_CLASS, AUTHZ_RESOURCE_MANAGER_HANDLE,
    AUTHZ_RM_FLAG_NO_AUDIT, AuthzAccessCheck, AuthzContextInfoGroupsSids, AuthzContextInfoUserSid,
    AuthzFreeContext, AuthzFreeResourceManager, AuthzGetInformationFromContext,
    AuthzInitializeContextFromSid, AuthzInitializeContextFromToken, AuthzInitializeResourceManager,
    AuthzSetAppContainerInformation, ConvertStringSidToSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetLengthSid,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES, TOKEN_GROUPS,
    TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::PCWSTR;

use super::Agent;

const FILE_WRITE_DATA: u32 = 0x2; // FILE_ADD_FILE on a folder
const FILE_APPEND_DATA: u32 = 0x4; // FILE_ADD_SUBDIRECTORY on a folder
const FILE_DELETE_CHILD: u32 = 0x40;
const DELETE: u32 = 0x1_0000;
const WRITE_DAC: u32 = 0x4_0000;
const WRITE_OWNER: u32 = 0x8_0000;
const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
/// A group that only denies (`SE_GROUP_USE_FOR_DENY_ONLY`): owner rights never flow through it.
const DENY_ONLY: u32 = 0x10;

/// The rights that let an identity rewrite, replace or re-permission a file, or add, remove or
/// rename a folder's entries: write and append (add a file or a folder), delete, delete a
/// child, `WRITE_DAC` and `WRITE_OWNER`. One mask for files and folders.
pub(crate) const CHANGE: u32 =
    FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_DELETE_CHILD | DELETE | WRITE_DAC | WRITE_OWNER;

/// The Authz contexts of the identities whose rights count, their resource manager, and the
/// SIDs (each as its bytes) whose ownership of a path counts as the agent's.
pub(crate) struct Writers {
    manager: AUTHZ_RESOURCE_MANAGER_HANDLE,
    contexts: Vec<AUTHZ_CLIENT_CONTEXT_HANDLE>,
    owners: Vec<Vec<u8>>,
}

impl Writers {
    /// The guard's own token, plus `agent`'s SIDs when given. `None` when any identity cannot be
    /// read: the caller then takes every path for modifiable.
    pub(crate) fn new(agent: &Agent) -> Option<Writers> {
        let mut writers = Writers::empty()?;
        writers.add_own_token()?;
        if let Some(user) = &agent.user {
            writers.add_sid(user, agent.package.as_deref())?;
        }
        Some(writers)
    }

    /// Only the SID `user` (inside `package` if given), not the guard's token: for the tests.
    #[cfg(test)]
    pub(crate) fn only_sid(user: &str, package: Option<&str>) -> Option<Writers> {
        let mut writers = Writers::empty()?;
        writers.add_sid(user, package)?;
        Some(writers)
    }

    fn empty() -> Option<Writers> {
        let mut manager = AUTHZ_RESOURCE_MANAGER_HANDLE::default();
        // SAFETY: no callbacks and no name; `manager` is written on success and freed in Drop.
        unsafe {
            AuthzInitializeResourceManager(
                AUTHZ_RM_FLAG_NO_AUDIT.0,
                None,
                None,
                None,
                PCWSTR::null(),
                &raw mut manager,
            )
        }
        .ok()?;
        Some(Writers {
            manager,
            contexts: Vec::new(),
            owners: Vec::new(),
        })
    }

    fn add_own_token(&mut self) -> Option<()> {
        let mut token = HANDLE::default();
        // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing; `token` is
        // written on success and closed below.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }.ok()?;
        let mut context = AUTHZ_CLIENT_CONTEXT_HANDLE::default();
        // SAFETY: `token` is the open token with query rights and `self.manager` the live
        // resource manager; `context` is written on success and freed in Drop.
        let made = unsafe {
            AuthzInitializeContextFromToken(
                0,
                token,
                self.manager,
                None,
                LUID::default(),
                None,
                &raw mut context,
            )
        };
        // SAFETY: `token` is the handle opened above, closed only here.
        drop(unsafe { CloseHandle(token) });
        made.ok()?;
        self.contexts.push(context);
        self.add_owners(context)
    }

    fn add_sid(&mut self, user: &str, package: Option<&str>) -> Option<()> {
        let user = Sid::parse(user)?;
        let package = match package {
            Some(text) => Some(Sid::parse(text)?),
            None => None,
        };
        let mut context = AUTHZ_CLIENT_CONTEXT_HANDLE::default();
        // SAFETY: `user.0` is a valid SID that outlives the call and `self.manager` the live
        // resource manager; flags 0 compute the SID's groups; `context` is written on success.
        unsafe {
            AuthzInitializeContextFromSid(
                0,
                user.0,
                self.manager,
                None,
                LUID::default(),
                None,
                &raw mut context,
            )
        }
        .ok()?;
        // Pushed now, so Drop frees it whatever follows.
        self.contexts.push(context);
        if let Some(package) = package {
            // SAFETY: `context` is the live context made above and `package.0` a valid SID that
            // outlives the call; no capabilities.
            unsafe { AuthzSetAppContainerInformation(context, package.0, None) }.ok()?;
            self.owners.push(sid_bytes(package.0)?);
        }
        self.add_owners(context)
    }

    /// The user and every group but the deny-only ones of `context`, as owners that count.
    fn add_owners(&mut self, context: AUTHZ_CLIENT_CONTEXT_HANDLE) -> Option<()> {
        let user = information(context, AuthzContextInfoUserSid)?;
        // SAFETY: AuthzContextInfoUserSid writes a TOKEN_USER at the buffer's start, which
        // `information` allocated 8-aligned and at least a TOKEN_GROUPS long, more than that.
        let user = unsafe { &*user.as_ptr().cast::<TOKEN_USER>() }.User.Sid;
        self.owners.push(sid_bytes(user)?);
        let groups = information(context, AuthzContextInfoGroupsSids)?;
        // SAFETY: as above, a TOKEN_GROUPS at the buffer's start.
        let list = unsafe { &*groups.as_ptr().cast::<TOKEN_GROUPS>() };
        // SAFETY: Authz wrote `GroupCount` SID_AND_ATTRIBUTES from `Groups` on, and the SIDs
        // they point at, all inside `groups`, which outlives `entries`.
        let entries: &[SID_AND_ATTRIBUTES] =
            unsafe { std::slice::from_raw_parts(list.Groups.as_ptr(), list.GroupCount as usize) };
        for entry in entries
            .iter()
            .filter(|entry| entry.Attributes & DENY_ONLY == 0)
        {
            self.owners.push(sid_bytes(entry.Sid)?);
        }
        Some(())
    }

    /// Whether any identity is granted any right of [`CHANGE`] on `path`; yes when anything
    /// fails.
    pub(crate) fn can_change(&self, path: &Path) -> bool {
        let Some(descriptor) = Descriptor::of(path) else {
            return true;
        };
        self.contexts
            .iter()
            .any(|&context| granted(context, &descriptor).is_none_or(|mask| mask & CHANGE != 0))
    }

    /// Whether `path`'s owner is one of the identities' users, packages or groups; yes when the
    /// owner cannot be read.
    pub(crate) fn owns(&self, path: &Path) -> bool {
        let Some(descriptor) = Descriptor::of(path) else {
            return true;
        };
        sid_bytes(descriptor.1).is_none_or(|owner| self.owners.contains(&owner))
    }
}

impl Drop for Writers {
    fn drop(&mut self) {
        for &context in &self.contexts {
            // SAFETY: each context was made by this value and is freed only here.
            drop(unsafe { AuthzFreeContext(context) });
        }
        // SAFETY: the manager was made by this value, freed only here, after its contexts.
        drop(unsafe { AuthzFreeResourceManager(self.manager) });
    }
}

/// The maximum `context` is granted against `descriptor`, or `None` when the check fails.
fn granted(context: AUTHZ_CLIENT_CONTEXT_HANDLE, descriptor: &Descriptor) -> Option<u32> {
    let request = AUTHZ_ACCESS_REQUEST {
        DesiredAccess: MAXIMUM_ALLOWED,
        ..AUTHZ_ACCESS_REQUEST::default()
    };
    let mut mask = 0_u32;
    let mut error = 0_u32;
    let mut reply = AUTHZ_ACCESS_REPLY {
        ResultListLength: 1,
        GrantedAccessMask: &raw mut mask,
        SaclEvaluationResults: std::ptr::null_mut(),
        Error: &raw mut error,
    };
    // SAFETY: `context` is live, `descriptor.0` a security descriptor GetNamedSecurityInfoW
    // returned and still owned, and `reply` points at one mask and one error, matching its
    // ResultListLength of 1; no audit event and no extra descriptors.
    unsafe {
        AuthzAccessCheck(
            AUTHZ_ACCESS_CHECK_FLAGS(0),
            context,
            &raw const request,
            None,
            descriptor.0,
            None,
            &raw mut reply,
            None,
        )
    }
    .ok()?;
    // A denial is a valid answer with a zero mask: nothing granted.
    Some(mask)
}

/// `class` of `context`, in a buffer aligned for the structures Authz writes there, or `None`.
fn information(
    context: AUTHZ_CLIENT_CONTEXT_HANDLE,
    class: AUTHZ_CONTEXT_INFORMATION_CLASS,
) -> Option<Vec<u64>> {
    let mut needed = 0_u32;
    // SAFETY: a size query with no buffer: it fails, with the size needed written to `needed`.
    drop(unsafe {
        AuthzGetInformationFromContext(context, class, 0, &raw mut needed, std::ptr::null_mut())
    });
    let bytes = usize::try_from(needed).ok()?;
    if bytes < std::mem::size_of::<TOKEN_GROUPS>() {
        return None;
    }
    let mut buffer = vec![0_u64; bytes.div_ceil(8)];
    // SAFETY: `buffer` holds at least `needed` bytes, 8-aligned, and outlives the call.
    unsafe {
        AuthzGetInformationFromContext(
            context,
            class,
            needed,
            &raw mut needed,
            buffer.as_mut_ptr().cast(),
        )
    }
    .ok()?;
    Some(buffer)
}

/// The bytes of the SID `sid` points at, or `None` for a null one.
fn sid_bytes(sid: PSID) -> Option<Vec<u8>> {
    if sid.0.is_null() {
        return None;
    }
    // SAFETY: `sid` points at a valid SID (Authz's, ConvertStringSidToSidW's or a security
    // descriptor's) whose owner outlives this call; GetLengthSid reads its own header.
    let length = usize::try_from(unsafe { GetLengthSid(sid) }).ok()?;
    // SAFETY: as above; the `length` bytes are the SID's, copied before this returns.
    let bytes = unsafe { std::slice::from_raw_parts(sid.0.cast::<u8>(), length) };
    Some(bytes.to_vec())
}

/// A SID `ConvertStringSidToSidW` allocated, freed on drop.
struct Sid(PSID);

impl Sid {
    fn parse(text: &str) -> Option<Sid> {
        let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
        let mut sid = PSID::default();
        // SAFETY: `wide` is NUL-terminated and outlives the call; `sid` is written on success.
        unsafe { ConvertStringSidToSidW(PCWSTR(wide.as_ptr()), &raw mut sid) }.ok()?;
        Some(Sid(sid))
    }
}

impl Drop for Sid {
    fn drop(&mut self) {
        // SAFETY: the SID was LocalAlloc'd by ConvertStringSidToSidW and is freed only here.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// A file's or folder's owner, group and DACL, as `GetNamedSecurityInfoW` allocated them, and the
/// owner, which points into the same allocation.
struct Descriptor(PSECURITY_DESCRIPTOR, PSID);

impl Descriptor {
    fn of(path: &Path) -> Option<Descriptor> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let mut owner = PSID::default();
        // SAFETY: `wide` is NUL-terminated and outlives the call; the descriptor is written on
        // success and freed in Drop, and `owner` points into it.
        let status = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(wide.as_ptr()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&raw mut owner),
                None,
                None,
                None,
                &raw mut descriptor,
            )
        };
        (status == ERROR_SUCCESS && !descriptor.0.is_null())
            .then_some(Descriptor(descriptor, owner))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was LocalAlloc'd by GetNamedSecurityInfoW, freed only here.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}

/// Tests only: the guard's own user SID as text.
#[cfg(test)]
pub(crate) fn own_user_sid() -> String {
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::core::PWSTR;
    let writers = Writers::new(&Agent::default()).expect("the guard's own token");
    let user = information(writers.contexts[0], AuthzContextInfoUserSid).expect("its user");
    // SAFETY: as in `add_owners`.
    let sid = unsafe { &*user.as_ptr().cast::<TOKEN_USER>() }.User.Sid;
    let mut text = PWSTR::null();
    // SAFETY: `sid` is valid while `user` lives; `text` is LocalAlloc'd on success, freed below.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.expect("the SID as text");
    // SAFETY: `text` is the NUL-terminated string just returned.
    let owned = unsafe { text.to_string() }.expect("UTF-16");
    // SAFETY: freed once, here.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    owned
}

/// Tests only: replaces `path`'s DACL with the protected one SDDL `dacl` writes (`D:P(...)`).
#[cfg(test)]
pub(crate) fn set_dacl(path: &Path, dacl: &str) {
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        SetNamedSecurityInfoW,
    };
    use windows::Win32::Security::{
        ACL, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    let sddl: Vec<u16> = dacl.encode_utf16().chain([0]).collect();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `sddl` is NUL-terminated; the descriptor is LocalAlloc'd on success, freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .expect("the SDDL");
    let mut present = windows::core::BOOL::default();
    let mut defaulted = windows::core::BOOL::default();
    let mut acl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `descriptor` is the live descriptor above; `acl` points into it.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &raw mut present,
            &raw mut acl,
            &raw mut defaulted,
        )
    }
    .expect("its DACL");
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is NUL-terminated and `acl` lives inside `descriptor`, freed after.
    let status = unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(wide.as_ptr()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(acl.cast_const()),
            None,
        )
    };
    // SAFETY: freed once, here, after its last use.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    assert_eq!(status, ERROR_SUCCESS, "setting the DACL");
}
