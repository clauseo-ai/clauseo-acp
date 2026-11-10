//! `sandbox_check` (`libsystem_sandbox`, part of libSystem; declared in `<sandbox.h>`'s private
//! half, not documented): whether a process runs under a profile, and whether its profile allows
//! an operation on a path. The launcher's outermost check and the guard's proof marker (0038)
//! use it.
//!
//! The guard (stage E's J2) mirrors these calls rather than depending on this crate, so their
//! meaning must stay stable: [`sandboxed`] and [`pid_sandboxed`] are `sandbox_check(pid, NULL,
//! SANDBOX_FILTER_NONE)` non-zero; [`path_allowed`] is `sandbox_check(pid, op,
//! SANDBOX_FILTER_PATH | SANDBOX_CHECK_NO_REPORT, path)` equal to 0, on a path that exists. A
//! change here is a change to the guard's proof marker too.
#![expect(
    unsafe_code,
    reason = "sandbox_check and getpid, which the crate has no binding for"
)]

use std::ffi::{CString, c_char, c_int};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// `SANDBOX_FILTER_NONE`: no filter argument follows.
const SANDBOX_FILTER_NONE: c_int = 0;
/// `SANDBOX_FILTER_PATH`: one `const char *` path follows.
const SANDBOX_FILTER_PATH: c_int = 1;
/// `SANDBOX_CHECK_NO_REPORT`, or'd into the filter: the check logs no denial.
const SANDBOX_CHECK_NO_REPORT: c_int = 0x4000_0000;

unsafe extern "C" {
    fn getpid() -> c_int;
    fn sandbox_check(pid: c_int, operation: *const c_char, filter: c_int, ...) -> c_int;
}

/// Whether this process runs under a Seatbelt profile. `sandbox_check` with no operation answers
/// 1 when sandboxed, 0 when not and -1 on an error; an error counts as sandboxed, the safe side.
pub fn sandboxed() -> bool {
    pid_sandboxed(0)
}

/// Whether process `pid` (0 for this one) runs under a Seatbelt profile, as [`sandboxed`]
/// answers for this one; an error counts as sandboxed. The guard asks it of Chrome's processes
/// from inside the browser profile: it answers 1 for each of them, the renderers included, and
/// needs no `process-info` rule (measured on macOS 27.2 with chrome-headless-shell 155).
pub fn pid_sandboxed(pid: i32) -> bool {
    // SAFETY: getpid takes nothing and always succeeds. sandbox_check reads no pointer when the
    // operation is null and the filter is SANDBOX_FILTER_NONE, which takes no variadic argument.
    unsafe {
        let pid = if pid == 0 { getpid() } else { pid };
        sandbox_check(pid, std::ptr::null(), SANDBOX_FILTER_NONE) != 0
    }
}

/// Whether the profile of process `pid` (0 for this one) allows `operation` (such as
/// `"file-write-data"`) on `path`, without logging a denial. `sandbox_check` answers 0 for an
/// allow; anything else (a deny, or an error) is `false`.
///
/// `path` must exist: the kernel answers 1 for a path that does not, even in a process with no
/// sandbox (measured on macOS 27.2, 2026-11-09). An operation or path holding a NUL is `false`.
pub fn path_allowed(pid: i32, operation: &str, path: &Path) -> bool {
    let (Ok(op), Ok(path)) = (
        CString::new(operation),
        CString::new(path.as_os_str().as_bytes()),
    ) else {
        return false;
    };
    // SAFETY: getpid always succeeds. op and path are NUL-terminated and outlive the call;
    // SANDBOX_FILTER_PATH takes exactly one `const char *` variadic argument.
    let answer = unsafe {
        let pid = if pid == 0 { getpid() } else { pid };
        sandbox_check(
            pid,
            op.as_ptr(),
            SANDBOX_FILTER_PATH | SANDBOX_CHECK_NO_REPORT,
            path.as_ptr(),
        )
    };
    answer == 0
}
