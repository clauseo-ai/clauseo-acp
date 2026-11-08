//! The token calls behind [`Owner`](super::Owner): this process's recorded parent and creation
//! time, the parent's token duplicated for impersonation, the impersonation itself, and what the
//! token gets on a file (`AccessCheck`, S15 `PR4i`). Every handle is closed by a guard.
#![expect(
    unsafe_code,
    reason = "FFI to the process, token and impersonation calls"
)]

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{
    AccessCheck, CheckTokenMembership, CreateRestrictedToken, DISABLE_MAX_PRIVILEGE,
    DuplicateTokenEx, GENERIC_MAPPING, GetSidSubAuthority, GetSidSubAuthorityCount,
    GetTokenInformation, ImpersonateLoggedOnUser, PRIVILEGE_SET, PSECURITY_DESCRIPTOR,
    RevertToSelf, SECURITY_IMPERSONATION_LEVEL, SID_AND_ATTRIBUTES, SecurityImpersonation,
    SetTokenInformation, TOKEN_ADJUST_DEFAULT, TOKEN_DUPLICATE, TOKEN_ELEVATION,
    TOKEN_ELEVATION_TYPE, TOKEN_IMPERSONATE, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL,
    TOKEN_QUERY, TokenElevation, TokenElevationType, TokenElevationTypeFull, TokenImpersonation,
    TokenImpersonationLevel, TokenIntegrityLevel,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
};
use windows::Win32::System::RemoteDesktop::WTSQueryUserToken;
use windows::Win32::System::Threading::{GetCurrentThread, OpenProcessToken, OpenThreadToken};
use windows::core::BOOL;

use super::super::accounts::LocalSid;
pub(super) use super::process::Process;
use super::{MEDIUM_RID, MachineError, needs_lowering};
use crate::windows::acl::object::words;
use crate::windows::record::user_of;
use crate::windows::sids::Sid;

/// The process ID of the parent Windows recorded for this process
/// (`InheritedFromUniqueProcessId`); 0 when none.
pub(super) fn recorded_parent() -> Result<u32, MachineError> {
    Process::current().recorded_parent()
}

/// This process's creation time, in `FILETIME` units.
pub(super) fn own_created() -> Result<u64, MachineError> {
    Process::current().created()
}

/// Makes the calling thread itself again. `false` when Windows refused.
pub(super) fn revert() -> bool {
    // SAFETY: no arguments; it ends whatever impersonation the calling thread has.
    unsafe { RevertToSelf() }.is_ok()
}

/// A Win32 failure as its call and code.
fn os(call: &'static str, error: &windows::core::Error) -> MachineError {
    MachineError::Os {
        call,
        code: error.code().0.cast_unsigned() & 0xFFFF,
    }
}

/// A token handle; closed on drop.
pub(super) struct Token(HANDLE);

impl Token {
    /// `process`'s token, duplicated as an impersonation token at `SecurityImpersonation`.
    pub(super) fn impersonation_of(process: &Process) -> Result<Token, MachineError> {
        let mut primary = HANDLE::default();
        // SAFETY: the process handle is open; `primary` is a live out-pointer, and the handle
        // it receives is owned by a `Token` at once.
        unsafe {
            OpenProcessToken(
                process.handle,
                TOKEN_QUERY | TOKEN_DUPLICATE,
                &raw mut primary,
            )
        }
        .map_err(|error| os("OpenProcessToken", &error))?;
        Token(primary).duplicate()
    }

    /// The token of the user signed in to `session` (`WTSQueryUserToken`, LOCAL SYSTEM only),
    /// duplicated as an impersonation token at `SecurityImpersonation`.
    pub(super) fn of_session(session: u32) -> Result<Token, MachineError> {
        let mut primary = HANDLE::default();
        // SAFETY: `primary` is a live out-pointer; the handle it receives is owned by a `Token`
        // at once.
        unsafe { WTSQueryUserToken(session, &raw mut primary) }
            .map_err(|error| os("WTSQueryUserToken", &error))?;
        let primary = Token(primary);
        primary.duplicate()
    }

    /// This token duplicated as an impersonation token at `SecurityImpersonation`, with the
    /// access the owner's calls need.
    fn duplicate(&self) -> Result<Token, MachineError> {
        let mut duplicate = HANDLE::default();
        // SAFETY: the token is open with TOKEN_DUPLICATE; no security attributes are passed;
        // `duplicate` is a live out-pointer, owned by a `Token` at once.
        unsafe {
            DuplicateTokenEx(
                self.0,
                TOKEN_QUERY | TOKEN_IMPERSONATE | TOKEN_DUPLICATE | TOKEN_ADJUST_DEFAULT,
                None,
                SecurityImpersonation,
                TokenImpersonation,
                &raw mut duplicate,
            )
        }
        .map_err(|error| os("DuplicateTokenEx", &error))?;
        Ok(Token(duplicate))
    }

    /// The token's user.
    pub(super) fn user(&self) -> Result<Sid, MachineError> {
        let text = user_of(self.0).map_err(|error| MachineError::Other(error.to_string()))?;
        Sid::parse(&text).map_err(|error| MachineError::Other(error.to_string()))
    }

    /// Makes the calling thread act as this token's user, then checks the thread's token names
    /// `sid` at `SecurityImpersonation` or above; on a failed check the thread goes back to
    /// itself.
    pub(super) fn impersonate(&self, sid: &Sid) -> Result<(), MachineError> {
        // SAFETY: the token is open with TOKEN_QUERY | TOKEN_IMPERSONATE.
        unsafe { ImpersonateLoggedOnUser(self.0) }
            .map_err(|error| os("ImpersonateLoggedOnUser", &error))?;
        let checked = Token::of_thread().and_then(|thread| {
            let level = thread.level()?;
            let user = thread.user()?;
            if level.0 < SecurityImpersonation.0 || user != *sid {
                return Err(MachineError::Other(
                    "Windows didn't let the set-up act as the owner".to_owned(),
                ));
            }
            Ok(())
        });
        if checked.is_err() && !revert() {
            // As in `Impersonating`'s drop: nothing safe is left.
            std::process::abort();
        }
        checked
    }

    /// Whether this token holds more than a standard user's ([`needs_lowering`]): a full
    /// elevated token, Administrators enabled, or an integrity level above Medium.
    pub(super) fn elevated(&self) -> Result<bool, MachineError> {
        let full = self.elevation_type()? == TokenElevationTypeFull;
        Ok(needs_lowering(
            full,
            self.admins_enabled()?,
            self.integrity()?,
        ))
    }

    /// A standard user's copy of this token: Administrators made deny-only and every privilege
    /// but `SeChangeNotifyPrivilege` deleted (`CreateRestrictedToken` with
    /// `DISABLE_MAX_PRIVILEGE`), then its integrity level set to Medium. Checked after: a copy
    /// with Administrators still enabled or above Medium is refused.
    pub(super) fn lowered(&self) -> Result<Token, MachineError> {
        let admins = LocalSid::parse(&sid(ADMINISTRATORS)?)?;
        let disable = [SID_AND_ATTRIBUTES {
            Sid: admins.0,
            Attributes: 0,
        }];
        let mut restricted = HANDLE::default();
        // SAFETY: the token is open with TOKEN_DUPLICATE; `disable` and the SID it points to
        // live for the call; `restricted` is a live out-pointer, owned by a `Token` at once.
        unsafe {
            CreateRestrictedToken(
                self.0,
                DISABLE_MAX_PRIVILEGE,
                Some(&disable),
                None,
                None,
                &raw mut restricted,
            )
        }
        .map_err(|error| os("CreateRestrictedToken", &error))?;
        let restricted = Token(restricted);
        let medium = LocalSid::parse(&sid(MEDIUM_LABEL)?)?;
        let label = TOKEN_MANDATORY_LABEL {
            Label: SID_AND_ATTRIBUTES {
                Sid: medium.0,
                Attributes: SE_GROUP_INTEGRITY,
            },
        };
        let size = u32::try_from(size_of::<TOKEN_MANDATORY_LABEL>()).unwrap_or(u32::MAX);
        // SAFETY: the restricted token has TOKEN_ADJUST_DEFAULT (it keeps this token's access);
        // `label` and the SID it points to live for the call.
        unsafe {
            SetTokenInformation(
                restricted.0,
                TokenIntegrityLevel,
                (&raw const label).cast(),
                size,
            )
        }
        .map_err(|error| os("SetTokenInformation", &error))?;
        if restricted.admins_enabled()? || restricted.integrity()? > MEDIUM_RID {
            return Err(MachineError::Other(
                "the owner's token couldn't be lowered to a standard user's".to_owned(),
            ));
        }
        Ok(restricted)
    }

    /// Whether Administrators is an enabled group of this (impersonation) token.
    fn admins_enabled(&self) -> Result<bool, MachineError> {
        let admins = LocalSid::parse(&sid(ADMINISTRATORS)?)?;
        let mut member = windows::core::BOOL::default();
        // SAFETY: the token is an impersonation token open with TOKEN_QUERY; the SID lives for
        // the call; `member` is a live out-pointer.
        unsafe { CheckTokenMembership(Some(self.0), admins.0, &raw mut member) }
            .map_err(|error| os("CheckTokenMembership", &error))?;
        Ok(member.as_bool())
    }

    /// The token's elevation type.
    fn elevation_type(&self) -> Result<TOKEN_ELEVATION_TYPE, MachineError> {
        let mut kind = TOKEN_ELEVATION_TYPE::default();
        self.fixed(TokenElevationType, &mut kind)?;
        Ok(kind)
    }

    /// The relative id of the token's integrity level (`S-1-16-<rid>`).
    fn integrity(&self) -> Result<u32, MachineError> {
        let mut buffer = [0u64; 16];
        let size = u32::try_from(size_of_val(&buffer)).unwrap_or(u32::MAX);
        let mut needed = 0u32;
        // SAFETY: the token is open with TOKEN_QUERY; `buffer` is live, aligned for a
        // TOKEN_MANDATORY_LABEL and `size` bytes long; the call writes the label and its SID
        // into it.
        unsafe {
            GetTokenInformation(
                self.0,
                TokenIntegrityLevel,
                Some(buffer.as_mut_ptr().cast()),
                size,
                &raw mut needed,
            )
        }
        .map_err(|error| os("GetTokenInformation", &error))?;
        // SAFETY: the call above filled `buffer` with a TOKEN_MANDATORY_LABEL whose SID points
        // into the buffer, which lives until the reads below are done; a label SID has one
        // sub-authority, its level, so index `count - 1` is in range.
        let rid = unsafe {
            let label = &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>();
            let count = *GetSidSubAuthorityCount(label.Label.Sid);
            *GetSidSubAuthority(label.Label.Sid, u32::from(count.saturating_sub(1)))
        };
        Ok(rid)
    }

    /// A fixed-size token information class read into `out`.
    fn fixed<T>(&self, class: TOKEN_INFORMATION_CLASS, out: &mut T) -> Result<(), MachineError> {
        let size = u32::try_from(size_of::<T>()).unwrap_or(u32::MAX);
        let mut needed = 0u32;
        // SAFETY: the token is open with TOKEN_QUERY; `out` is live and `size` bytes long,
        // and `class` is one whose answer has `T`'s layout.
        unsafe {
            GetTokenInformation(
                self.0,
                class,
                Some(std::ptr::from_mut(out).cast()),
                size,
                &raw mut needed,
            )
        }
        .map_err(|error| os("GetTokenInformation", &error))
    }

    /// The calling thread's token, opened with the process's own identity for the check.
    fn of_thread() -> Result<Token, MachineError> {
        let mut handle = HANDLE::default();
        // SAFETY: GetCurrentThread is a pseudo-handle that needs no close; `handle` is a live
        // out-pointer, owned by a `Token` at once.
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &raw mut handle) }
            .map_err(|error| os("OpenThreadToken", &error))?;
        Ok(Token(handle))
    }

    /// An impersonation token's level.
    fn level(&self) -> Result<SECURITY_IMPERSONATION_LEVEL, MachineError> {
        let mut level = SECURITY_IMPERSONATION_LEVEL::default();
        let size = u32::try_from(size_of::<SECURITY_IMPERSONATION_LEVEL>()).unwrap_or(u32::MAX);
        let mut needed = 0u32;
        // SAFETY: the token is open with TOKEN_QUERY; `level` is live and `size` bytes long, and
        // the call writes a SECURITY_IMPERSONATION_LEVEL into it.
        unsafe {
            GetTokenInformation(
                self.0,
                TokenImpersonationLevel,
                Some((&raw mut level).cast()),
                size,
                &raw mut needed,
            )
        }
        .map_err(|error| os("GetTokenInformation", &error))?;
        Ok(level)
    }

    /// What this token gets on a file whose self-relative descriptor (owner, group, DACL and
    /// label) is `descriptor`: `AccessCheck` asked for `MAXIMUM_ALLOWED` with the files' generic
    /// mapping, so the integrity label counts as it does at an open (S15 `PR4i`). Nothing
    /// granted is 0.
    pub(super) fn granted(&self, descriptor: &[u8]) -> Result<u32, MachineError> {
        /// `MAXIMUM_ALLOWED`: every right the descriptor gives the token.
        const MAXIMUM_ALLOWED: u32 = 0x0200_0000;
        let mapping = GENERIC_MAPPING {
            GenericRead: FILE_GENERIC_READ.0,
            GenericWrite: FILE_GENERIC_WRITE.0,
            GenericExecute: FILE_GENERIC_EXECUTE.0,
            GenericAll: FILE_ALL_ACCESS.0,
        };
        let mut aligned = words(descriptor);
        // Room for the privileges the check reports using: far more than it ever lists.
        let mut used = [0_u64; 32];
        let mut length = u32::try_from(size_of_val(&used)).unwrap_or(0);
        let (mut granted, mut status) = (0_u32, BOOL(0));
        // SAFETY: `aligned` holds a valid self-relative descriptor and lives for the call; the
        // token is an impersonation token open with TOKEN_QUERY; the mapping, `used` (`length`
        // bytes) and the out-pointers are live.
        unsafe {
            AccessCheck(
                PSECURITY_DESCRIPTOR(aligned.as_mut_ptr().cast()),
                self.0,
                MAXIMUM_ALLOWED,
                &raw const mapping,
                Some(used.as_mut_ptr().cast::<PRIVILEGE_SET>()),
                &raw mut length,
                &raw mut granted,
                &raw mut status,
            )
        }
        .map_err(|error| os("AccessCheck", &error))?;
        Ok(if status.as_bool() { granted } else { 0 })
    }
}

/// `BUILTIN\Administrators`.
const ADMINISTRATORS: &str = "S-1-5-32-544";
/// The Medium mandatory level.
const MEDIUM_LABEL: &str = "S-1-16-8192";
/// `SE_GROUP_INTEGRITY`.
const SE_GROUP_INTEGRITY: u32 = 0x20;
/// LOCAL SYSTEM.
const SYSTEM: &str = "S-1-5-18";

/// `text` as a SID.
fn sid(text: &str) -> Result<Sid, MachineError> {
    Sid::parse(text).map_err(|error| MachineError::Other(error.to_string()))
}

/// This process's token: whether it is elevated (`TokenElevation`) and whether its user is
/// LOCAL SYSTEM, for `--per-machine`'s check.
pub(crate) fn this_process() -> Result<(bool, bool), MachineError> {
    let token = Token::impersonation_of(&Process::current())?;
    let mut elevation = TOKEN_ELEVATION::default();
    token.fixed(TokenElevation, &mut elevation)?;
    Ok((
        elevation.TokenIsElevated != 0,
        token.user()?.as_str() == SYSTEM,
    ))
}

/// Whether the calling thread's token is no stronger than a standard user's: Administrators
/// not enabled, Medium or lower. For the tests of the lowered owner.
#[cfg(test)]
pub(crate) fn thread_is_standard() -> bool {
    Token::of_thread().is_ok_and(|thread| {
        !thread.admins_enabled().unwrap_or(true)
            && thread.integrity().unwrap_or(u32::MAX) <= MEDIUM_RID
    })
}

/// How many privileges the calling thread's token holds besides `SeChangeNotifyPrivilege`,
/// enabled or not: for the tests of the lowered owner. `None` when it can't be read.
#[cfg(test)]
pub(crate) fn thread_extra_privileges() -> Option<usize> {
    use windows::Win32::Security::{
        LookupPrivilegeValueW, SE_CHANGE_NOTIFY_NAME, TOKEN_PRIVILEGES, TokenPrivileges,
    };

    let thread = Token::of_thread().ok()?;
    let mut buffer = [0u64; 512];
    let size = u32::try_from(size_of_val(&buffer)).ok()?;
    let mut needed = 0u32;
    // SAFETY: the token is open with TOKEN_QUERY; `buffer` is live, aligned for a
    // TOKEN_PRIVILEGES and `size` bytes long.
    unsafe {
        GetTokenInformation(
            thread.0,
            TokenPrivileges,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &raw mut needed,
        )
    }
    .ok()?;
    let mut notify = windows::Win32::Foundation::LUID::default();
    // SAFETY: no system name; the privilege's name is a static NUL-terminated string; `notify`
    // is a live out-pointer.
    unsafe {
        LookupPrivilegeValueW(
            windows::core::PCWSTR::null(),
            SE_CHANGE_NOTIFY_NAME,
            &raw mut notify,
        )
    }
    .ok()?;
    // SAFETY: the call above filled `buffer` with a TOKEN_PRIVILEGES whose `PrivilegeCount`
    // entries follow it inside the buffer, which outlives the slice.
    let privileges = unsafe {
        let list = buffer.as_ptr().cast::<TOKEN_PRIVILEGES>();
        let count = usize::try_from((*list).PrivilegeCount).ok()?;
        std::slice::from_raw_parts((*list).Privileges.as_ptr(), count)
    };
    let is_notify = |luid: &windows::Win32::Foundation::LUID| {
        luid.LowPart == notify.LowPart && luid.HighPart == notify.HighPart
    };
    Some(privileges.iter().filter(|p| !is_notify(&p.Luid)).count())
}

/// Whether the calling thread holds an impersonation token: for the tests of the guard.
#[cfg(test)]
pub(crate) fn thread_has_token() -> bool {
    Token::of_thread().is_ok()
}

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcessToken, DuplicateTokenEx or OpenThreadToken
        // and is closed once, here. A failed close leaves nothing to do.
        drop(unsafe { CloseHandle(self.0) });
    }
}
