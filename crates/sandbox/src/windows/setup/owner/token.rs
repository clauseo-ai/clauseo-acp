//! The token calls behind [`Owner`](super::Owner): this process's recorded parent and creation
//! time, the parent's token duplicated for impersonation, and the impersonation itself. Every
//! handle is closed by a guard.
#![expect(
    unsafe_code,
    reason = "FFI to the process, token and impersonation calls"
)]

use std::ffi::c_void;

use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::Security::{
    DuplicateTokenEx, GetTokenInformation, ImpersonateLoggedOnUser, RevertToSelf,
    SECURITY_IMPERSONATION_LEVEL, SecurityImpersonation, TOKEN_DUPLICATE, TOKEN_IMPERSONATE,
    TOKEN_QUERY, TokenImpersonation, TokenImpersonationLevel,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, GetProcessTimes, OpenProcess, OpenProcessToken,
    OpenThreadToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::MachineError;
use crate::windows::record::user_of;
use crate::windows::sids::Sid;

/// `PROCESS_BASIC_INFORMATION`, declared here so the `windows` crate's `Win32_System_Kernel`
/// (for its `PEB` pointer) isn't needed; the same `repr(C)` layout.
#[repr(C)]
#[derive(Default)]
struct BasicInformation {
    exit_status: i32,
    peb_base_address: usize,
    affinity_mask: usize,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
}

/// The process ID of the parent Windows recorded for this process
/// (`InheritedFromUniqueProcessId`); 0 when none.
pub(super) fn recorded_parent() -> Result<u32, MachineError> {
    let mut info = BasicInformation::default();
    let size = u32::try_from(size_of::<BasicInformation>()).unwrap_or(u32::MAX);
    let mut written = 0u32;
    // SAFETY: GetCurrentProcess is a pseudo-handle that needs no close; `info` is a live,
    // writable struct of `size` bytes with PROCESS_BASIC_INFORMATION's layout; `written` is a
    // live out-pointer.
    let status = unsafe {
        NtQueryInformationProcess(
            GetCurrentProcess(),
            ProcessBasicInformation,
            (&raw mut info).cast::<c_void>(),
            size,
            &raw mut written,
        )
    };
    if status.is_err() {
        return Err(MachineError::Os {
            call: "NtQueryInformationProcess",
            code: status.0.cast_unsigned(),
        });
    }
    u32::try_from(info.inherited_from_unique_process_id)
        .map_err(|error| MachineError::Other(format!("the recorded parent's process ID: {error}")))
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

/// A process handle; closed on drop unless it is the current process's pseudo-handle.
pub(super) struct Process {
    handle: HANDLE,
    owned: bool,
}

impl Process {
    /// Opens process `pid` for `PROCESS_QUERY_LIMITED_INFORMATION`: enough for its times and to
    /// open its token. Holding the handle keeps the process object, so its ID can't be reused
    /// while it is open.
    pub(super) fn open(pid: u32) -> Result<Process, MachineError> {
        // SAFETY: no pointers; the handle is owned by `Process` from here.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|error| os("OpenProcess", &error))?;
        Ok(Process {
            handle,
            owned: true,
        })
    }

    /// This process, through its pseudo-handle.
    pub(super) fn current() -> Process {
        Process {
            // SAFETY: no arguments; the pseudo-handle needs no close.
            handle: unsafe { GetCurrentProcess() },
            owned: false,
        }
    }

    /// Its creation time, in `FILETIME` units.
    pub(super) fn created(&self) -> Result<u64, MachineError> {
        let mut times = [FILETIME::default(); 4];
        let [created, exited, kernel, user] = &mut times;
        // SAFETY: the handle is open; the four out-pointers are live and distinct.
        unsafe {
            GetProcessTimes(
                self.handle,
                &raw mut *created,
                &raw mut *exited,
                &raw mut *kernel,
                &raw mut *user,
            )
        }
        .map_err(|error| os("GetProcessTimes", &error))?;
        Ok(u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: the handle came from OpenProcess and is closed once, here. A failed close
            // leaves nothing to do.
            drop(unsafe { CloseHandle(self.handle) });
        }
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
        let primary = Token(primary);
        let mut duplicate = HANDLE::default();
        // SAFETY: `primary` is an open token with TOKEN_DUPLICATE; no security attributes are
        // passed; `duplicate` is a live out-pointer, owned by a `Token` at once.
        unsafe {
            DuplicateTokenEx(
                primary.0,
                TOKEN_QUERY | TOKEN_IMPERSONATE | TOKEN_DUPLICATE,
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
