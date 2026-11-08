//! Whether a path's descriptor refuses a pool user a read or a write (0049, "The denial
//! reporter, client side"), worked out as the lawyer, unelevated, with `AuthzAccessCheck`.
//!
//! - A resource manager with no audit (`AUTHZ_RM_FLAG_NO_AUDIT`), and a client context per pool
//!   user from its SID alone (`AuthzInitializeContextFromSid`, flags 0, so its local groups are
//!   in it: `Users`, the pool's group), kept for later checks.
//! - Only a local drive-letter path is checked (`watch_client::is_local_drive_path`), on a drive
//!   Windows doesn't call remote (`GetDriveTypeW` of its root, which opens nothing on it): a UNC,
//!   `\\?\`, `\\.\` or device path, or a letter mapped to a share, is `None` before anything is
//!   opened, so no check reaches the network.
//! - The path is opened one part at a time (`watch_client::open_no_reparse`): the drive's root
//!   by name, then each part relative to the folder's handle before it (`object::open_at`,
//!   `NtCreateFile` with `OBJ_DONT_REPARSE` and `FILE_OPEN_REPARSE_POINT`, backup intent so a
//!   folder opens too). No part is ever followed, so a folder a pool user swaps for a link to a
//!   share mid-walk fails the open rather than reaching the network. A part that is itself a
//!   reparse point (a junction, a symbolic link, a mount point), the last included, is `None`.
//!   No full path is opened. The owner, group and DACL are read on the last part's handle
//!   (`READ_CONTROL`).
//! - The check asks for the data right: `FILE_READ_DATA` (a folder's `FILE_LIST_DIRECTORY`) for a
//!   read, `FILE_WRITE_DATA` (`FILE_ADD_FILE`) for a write. Metadata isn't asked, as macOS
//!   doesn't report it.
//!
//! It models the DACL only: not the agent's Low label, nor its token's restricting SIDs, so a
//! refusal the label or the restricting set made reads as allowed here. `None` is "can't tell":
//! a path that doesn't open (gone, or this user may not read its descriptor), or a failed call.
#![expect(unsafe_code, reason = "FFI to the Authz calls")]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Mutex, PoisonError};

use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, LUID};
use windows::Win32::Security::Authorization::{
    AUTHZ_ACCESS_CHECK_FLAGS, AUTHZ_ACCESS_REPLY, AUTHZ_ACCESS_REQUEST,
    AUTHZ_CLIENT_CONTEXT_HANDLE, AUTHZ_RESOURCE_MANAGER_HANDLE, AUTHZ_RM_FLAG_NO_AUDIT,
    AuthzAccessCheck, AuthzFreeContext, AuthzFreeResourceManager, AuthzInitializeContextFromSid,
    AuthzInitializeResourceManager,
};
use windows::Win32::Security::{
    DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
    FILE_WRITE_DATA, FileAttributeTagInfo, GetDriveTypeW, GetFileInformationByHandleEx,
    READ_CONTROL,
};
use windows::core::{HSTRING, PCWSTR};

use super::object::{Handle, kernel_security, open_at, open_raw, words};
use crate::Access;
use crate::windows::folder::verbatim_wide;
use crate::windows::sids::Sid;
use crate::windows::token::os::LocalSid;
use crate::windows::watch_client::{Walker, is_local_drive_path, open_no_reparse};

/// `DRIVE_REMOTE`, `DRIVE_NO_ROOT_DIR` and `DRIVE_UNKNOWN`: drives the check doesn't open.
const NOT_LOCAL: [u32; 3] = [4, 1, 0];

/// Whether `path` is a local drive-letter path on a drive that isn't remote.
fn local(path: &Path) -> bool {
    if !is_local_drive_path(path) {
        return false;
    }
    let Some(root) = path.to_str().and_then(|text| text.get(..3)) else {
        return false;
    };
    // SAFETY: a live HSTRING naming a drive's root; the call opens nothing on it.
    let kind = unsafe { GetDriveTypeW(&HSTRING::from(root)) };
    !NOT_LOCAL.contains(&kind)
}

/// The opens of [`open_no_reparse`], on the file system.
struct NtWalker;

impl Walker for NtWalker {
    type Handle = Handle;

    fn root(&mut self, root: &str) -> Option<Handle> {
        let wide = verbatim_wide(Path::new(root)).ok()?;
        open_raw(&wide, READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0).ok()
    }

    fn child(&mut self, dir: &Handle, name: &str, last: bool) -> Option<(Handle, bool)> {
        let wide: Vec<u16> = name.encode_utf16().collect();
        let access = if last {
            READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0
        } else {
            FILE_READ_ATTRIBUTES.0
        };
        // A reparse point met on the way fails the open; one at `name` is opened itself.
        let handle = open_at(dir, &wide, access, false).ok()?;
        let reparse = is_reparse(&handle)?;
        Some((handle, reparse))
    }
}

/// Whether `handle` is open on a reparse point; `None` when its attributes can't be read.
fn is_reparse(handle: &Handle) -> Option<bool> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: the handle is open and `tag` is valid and writable for its size.
    unsafe {
        GetFileInformationByHandleEx(
            handle.0,
            FileAttributeTagInfo,
            (&raw mut tag).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(0),
        )
    }
    .ok()?;
    Some(tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 || tag.ReparseTag != 0)
}

/// A resource manager and the pool users' contexts, freed on drop.
pub(in crate::windows) struct Checker {
    manager: AUTHZ_RESOURCE_MANAGER_HANDLE,
    contexts: Mutex<BTreeMap<Sid, AUTHZ_CLIENT_CONTEXT_HANDLE>>,
}

// SAFETY: Authz handles may be used from any thread; every use of a context is under the
// mutex, and the manager is only read after it is made.
unsafe impl Send for Checker {}
// SAFETY: as for Send.
unsafe impl Sync for Checker {}

impl Checker {
    /// A checker with no audit; `None` when the resource manager can't be made.
    pub(in crate::windows) fn new() -> Option<Checker> {
        let mut manager = AUTHZ_RESOURCE_MANAGER_HANDLE::default();
        // SAFETY: no callbacks and no name; `manager` is a live out-pointer, freed on drop.
        let made = unsafe {
            AuthzInitializeResourceManager(
                AUTHZ_RM_FLAG_NO_AUDIT.0,
                None,
                None,
                None,
                PCWSTR::null(),
                &raw mut manager,
            )
        };
        if let Err(error) = made {
            tracing::debug!(
                code = error.code().0,
                "AuthzInitializeResourceManager failed"
            );
            return None;
        }
        Some(Checker {
            manager,
            contexts: Mutex::new(BTreeMap::new()),
        })
    }

    /// Whether `path`'s descriptor refuses `access` to `pool_user`; `None` when it can't tell.
    pub(in crate::windows) fn would_deny(
        &self,
        pool_user: &Sid,
        path: &Path,
        access: Access,
    ) -> Option<bool> {
        if !local(path) {
            return None;
        }
        let handle = open_no_reparse(path, &mut NtWalker)?;
        let info = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION;
        let descriptor = kernel_security(&handle, (info | DACL_SECURITY_INFORMATION).0).ok()?;
        drop(handle);
        let mut descriptor = words(&descriptor);
        let wanted = match access {
            Access::Read => FILE_READ_DATA.0,
            Access::Write => FILE_WRITE_DATA.0,
        };
        let mut contexts = self.contexts.lock().unwrap_or_else(PoisonError::into_inner);
        let context = if let Some(context) = contexts.get(pool_user) {
            *context
        } else {
            let context = self.context(pool_user)?;
            contexts.insert(pool_user.clone(), context);
            context
        };
        let request = AUTHZ_ACCESS_REQUEST {
            DesiredAccess: wanted,
            ..AUTHZ_ACCESS_REQUEST::default()
        };
        let mut granted = 0u32;
        let mut error = 0u32;
        let mut reply = AUTHZ_ACCESS_REPLY {
            ResultListLength: 1,
            GrantedAccessMask: &raw mut granted,
            SaclEvaluationResults: std::ptr::null_mut(),
            Error: &raw mut error,
        };
        // SAFETY: a live context (under the lock) and request; the descriptor is self-relative,
        // aligned in `descriptor`, which outlives the call; the reply's one-entry lists point at
        // live locals.
        let checked = unsafe {
            AuthzAccessCheck(
                AUTHZ_ACCESS_CHECK_FLAGS(0),
                context,
                &raw const request,
                None,
                PSECURITY_DESCRIPTOR(descriptor.as_mut_ptr().cast()),
                None,
                &raw mut reply,
                None,
            )
        };
        drop(contexts);
        if let Err(failed) = checked {
            tracing::debug!(code = failed.code().0, "AuthzAccessCheck failed");
            return None;
        }
        match error {
            0 => Some(granted & wanted != wanted),
            code if code == ERROR_ACCESS_DENIED.0 => Some(true),
            _ => None,
        }
    }

    /// A client context for `pool_user` with its local groups; `None` when it can't be made.
    fn context(&self, pool_user: &Sid) -> Option<AUTHZ_CLIENT_CONTEXT_HANDLE> {
        let sid = LocalSid::new(pool_user).ok()?;
        let mut context = AUTHZ_CLIENT_CONTEXT_HANDLE::default();
        // SAFETY: a live manager and SID; no expiry and no dynamic groups; `context` is a live
        // out-pointer, freed with the checker.
        let made = unsafe {
            AuthzInitializeContextFromSid(
                0,
                sid.as_psid(),
                self.manager,
                None,
                LUID::default(),
                None,
                &raw mut context,
            )
        };
        match made {
            Ok(()) => Some(context),
            Err(error) => {
                tracing::debug!(
                    code = error.code().0,
                    "AuthzInitializeContextFromSid failed"
                );
                None
            }
        }
    }
}

impl Drop for Checker {
    fn drop(&mut self) {
        let contexts = self
            .contexts
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        for context in std::mem::take(contexts).into_values() {
            // SAFETY: made by AuthzInitializeContextFromSid, freed once, here.
            if let Err(error) = unsafe { AuthzFreeContext(context) } {
                tracing::debug!(code = error.code().0, "AuthzFreeContext failed");
            }
        }
        // SAFETY: made by AuthzInitializeResourceManager, freed once, after its contexts.
        if let Err(error) = unsafe { AuthzFreeResourceManager(self.manager) } {
            tracing::debug!(code = error.code().0, "AuthzFreeResourceManager failed");
        }
    }
}

/// Whether `path`'s descriptor refuses `access` to the pool user `pool_user`, with a checker of
/// its own; `None` when it can't tell.
pub(in crate::windows) fn would_deny(pool_user: &Sid, path: &Path, access: Access) -> Option<bool> {
    if !local(path) {
        return None;
    }
    Checker::new()?.would_deny(pool_user, path, access)
}

#[cfg(test)]
mod tests;
