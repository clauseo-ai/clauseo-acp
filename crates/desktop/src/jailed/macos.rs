//! macOS: asks the Seatbelt sandbox about this process with `sandbox_check` (`libsystem_sandbox`,
//! part of libSystem; declared in `<sandbox.h>` but not documented). With no operation it answers
//! whether the process has a sandbox at all.
#![expect(
    unsafe_code,
    reason = "sandbox_check and getpid, which the crate has no binding for"
)]

use std::ffi::{c_char, c_int};

/// `SANDBOX_FILTER_NONE` of `enum sandbox_filter_type`: no filter argument follows.
const SANDBOX_FILTER_NONE: c_int = 0;

unsafe extern "C" {
    fn getpid() -> c_int;
    fn sandbox_check(pid: c_int, operation: *const c_char, filter: c_int, ...) -> c_int;
}

/// Whether this process runs under a Seatbelt profile: `sandbox_check` with no operation is
/// non-zero. It returns 1 when sandboxed, 0 when not, and -1 on an error; -1 is non-zero, so an
/// error counts as sandboxed (the safe side), and this never fails open.
pub(super) fn sandboxed() -> bool {
    // SAFETY: getpid takes nothing and always succeeds. sandbox_check reads no pointer when the
    // operation is null and the filter is SANDBOX_FILTER_NONE, which takes no variadic argument.
    let answer = unsafe { sandbox_check(getpid(), std::ptr::null(), SANDBOX_FILTER_NONE) };
    answer != 0
}
