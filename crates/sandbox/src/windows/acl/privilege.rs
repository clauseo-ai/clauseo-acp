//! Privileges enabled in this process's token for as long as a guard lives (S15 `PR4i`), as
//! `setup/processes.rs` does for `SeDebugPrivilege`: `--reclaim` enables
//! `SeTakeOwnershipPrivilege` and `SeRestorePrivilege` (an owner write naming another user than
//! this one), and `SeBackupPrivilege` (reading the owner and listing a folder that shuts the
//! lawyer out; neither of the other two grants `READ_CONTROL`). Each is enabled in turn; on drop,
//! and when one can't be, the token gets back the state each had before. A token that doesn't
//! hold one (an unelevated process) is refused, naming the privilege.
#![expect(unsafe_code, reason = "FFI to the token's privileges")]

use std::io;

use windows::Win32::Foundation::{ERROR_NOT_ALL_ASSIGNED, GetLastError, HANDLE, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PCWSTR};

use super::object::{Handle, os_error};

/// What `--reclaim` enables.
pub(in crate::windows) const RECLAIM: [&str; 3] = [
    "SeTakeOwnershipPrivilege",
    "SeRestorePrivilege",
    "SeBackupPrivilege",
];

/// Privileges enabled for as long as this lives.
#[derive(Debug)]
pub(in crate::windows) struct Privileges {
    /// The process token, open for `TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY`.
    token: Handle,
    /// Each privilege's state before it was enabled, in order; put back in reverse on drop.
    previous: Vec<TOKEN_PRIVILEGES>,
}

impl Privileges {
    /// Enables each of `names` in this process's token, as the module says.
    pub(in crate::windows) fn enable(names: &[&str]) -> io::Result<Privileges> {
        let mut token = HANDLE::default();
        let access = TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY;
        // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `token` is a
        // live out-pointer, owned by `Handle` once written.
        unsafe { OpenProcessToken(GetCurrentProcess(), access, &raw mut token) }
            .map_err(|error| os_error(&error))?;
        let mut held = Privileges {
            token: Handle(token),
            previous: Vec::new(),
        };
        for name in names {
            // On a refusal `held` drops here, and puts back those already enabled.
            held.enable_one(name)?;
        }
        Ok(held)
    }

    /// Enables `name`, keeping its state before.
    fn enable_one(&mut self, name: &str) -> io::Result<()> {
        let named = |error: io::Error| io::Error::new(error.kind(), format!("{name}: {error}"));
        let mut luid = LUID::default();
        let wide = HSTRING::from(name);
        // SAFETY: no system name (this one); the name is NUL-terminated and outlives the call;
        // `luid` is a live out-pointer.
        unsafe { LookupPrivilegeValueW(PCWSTR::null(), &wide, &raw mut luid) }
            .map_err(|error| named(os_error(&error)))?;
        let wanted = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let mut previous = TOKEN_PRIVILEGES::default();
        let size = u32::try_from(size_of::<TOKEN_PRIVILEGES>()).unwrap_or(0);
        let mut returned = 0u32;
        // SAFETY: the token is open with TOKEN_ADJUST_PRIVILEGES; `wanted` and `previous` are
        // live and `size` bytes long (one privilege each); `returned` is a live out-pointer.
        let adjusted = unsafe {
            AdjustTokenPrivileges(
                self.token.0,
                false,
                Some(&raw const wanted),
                size,
                Some(&raw mut previous),
                Some(&raw mut returned),
            )
        };
        // SAFETY: reads this thread's last error, which the call just made set.
        let last = unsafe { GetLastError() };
        adjusted.map_err(|error| named(os_error(&error)))?;
        // A privilege the token lacked changed nothing: `previous` then lists none.
        if previous.PrivilegeCount > 0 {
            self.previous.push(previous);
        }
        if last == ERROR_NOT_ALL_ASSIGNED {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "this process's token doesn't hold {name}: the clean-up runs only as an \
                     administrator"
                ),
            ));
        }
        Ok(())
    }
}

impl Drop for Privileges {
    fn drop(&mut self) {
        let size = u32::try_from(size_of::<TOKEN_PRIVILEGES>()).unwrap_or(0);
        for previous in self.previous.iter().rev() {
            // SAFETY: the token is still open with TOKEN_ADJUST_PRIVILEGES (it drops after
            // this); `previous` is live and `size` bytes long; no previous state is asked for.
            let restored = unsafe {
                AdjustTokenPrivileges(
                    self.token.0,
                    false,
                    Some(&raw const *previous),
                    size,
                    None,
                    None,
                )
            };
            if let Err(error) = restored {
                tracing::warn!(code = error.code().0, "a privilege's state wasn't restored");
            }
        }
    }
}

#[cfg(test)]
mod tests;
