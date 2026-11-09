//! The sweep's one private call, `sandbox_check` from `libsystem_sandbox` (re-exported
//! by `libSystem`), with its `unsafe` blocks and why each is sound. Undocumented, but stable for
//! years and used by Chromium and `WebKit`; the drift check (`sweep.rs`) turns a change in its
//! answers into a loud failure.
#![expect(
    unsafe_code,
    reason = "FFI: sandbox_check and its SANDBOX_CHECK_NO_REPORT flag for the session sweep, getuid"
)]

use std::ffi::{CStr, c_char, c_int};

use super::Checker;

unsafe extern "C" {
    /// `int sandbox_check(pid_t pid, const char *operation, enum sandbox_filter_type type, ...)`:
    /// 0 allowed, 1 denied, -1 failed (`errno`). With `SANDBOX_FILTER_PATH` the variadic
    /// argument is a `const char *` path.
    fn sandbox_check(pid: libc::pid_t, operation: *const c_char, filter: c_int, ...) -> c_int;
    /// The flag that keeps a check out of the log: without it every "denied" answer is logged
    /// as the process's own denial (about 700 lines a scan, measured), which the watcher would
    /// report. Exported as a constant; its value is read here.
    static SANDBOX_CHECK_NO_REPORT: c_int;
}

/// `SANDBOX_FILTER_NONE`: the question is whether the process is sandboxed at all.
const FILTER_NONE: c_int = 0;
/// `SANDBOX_FILTER_PATH`, an enum value `libsystem_sandbox` does not export (1, measured).
const FILTER_PATH: c_int = 1;

/// The real kernel's answers.
pub(super) struct Seatbelt;

impl Checker for Seatbelt {
    fn sandboxed(&self, pid: u32) -> Option<bool> {
        let pid = libc::pid_t::try_from(pid).ok()?;
        // SAFETY: a plain pid, a null operation (allowed with FILTER_NONE, which reads no
        // variadic argument) and an integer flag; the call reads nothing else.
        let answer = unsafe { sandbox_check(pid, std::ptr::null(), FILTER_NONE | no_report()) };
        match answer {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        }
    }

    fn may_write(&self, pid: u32, path: &CStr) -> Option<bool> {
        let pid = libc::pid_t::try_from(pid).ok()?;
        // SAFETY: the operation is a NUL-terminated literal; with FILTER_PATH the call reads
        // exactly one variadic argument, a NUL-terminated path, which `path` is and outlives
        // the call.
        let answer = unsafe {
            sandbox_check(
                pid,
                c"file-write-data".as_ptr(),
                FILTER_PATH | no_report(),
                path.as_ptr(),
            )
        };
        match answer {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }
}

/// `SANDBOX_CHECK_NO_REPORT`'s value.
fn no_report() -> c_int {
    // SAFETY: an exported, initialised constant of libsystem_sandbox, read and never written.
    unsafe { SANDBOX_CHECK_NO_REPORT }
}

/// This process's real user id: the agents run as it.
pub(super) fn uid() -> u32 {
    // SAFETY: getuid(2) takes nothing and cannot fail.
    unsafe { libc::getuid() }
}
