//! The sweep's one private call, `sandbox_check` from `libsystem_sandbox` (re-exported by
//! `libSystem`), with its `unsafe` blocks and why each is sound. Undocumented, but stable for
//! years and used by Chromium and `WebKit`. It is looked up at run time (`dlsym`), never linked:
//! if a macOS update removes it or its `SANDBOX_CHECK_NO_REPORT` flag, the host still loads and
//! jailed launches are refused ([`api`], `SweepError::Unavailable`). A change in its answers is
//! caught by the launch's sentinel and the drift check (`sweep.rs`).
#![expect(
    unsafe_code,
    reason = "FFI: dlsym of sandbox_check and SANDBOX_CHECK_NO_REPORT, the calls, getuid, lchflags"
)]

use std::ffi::{CStr, c_char, c_int, c_void};
use std::io;
use std::sync::OnceLock;

use super::{Answer, CheckFailed, Checker, ProcId, SweepError, start_of};

/// `int sandbox_check(pid_t pid, const char *operation, enum sandbox_filter_type type, ...)`:
/// 0 allowed, 1 denied, -1 failed (`errno`). With `SANDBOX_FILTER_PATH` the variadic argument is
/// a `const char *` path.
type SandboxCheck = unsafe extern "C" fn(libc::pid_t, *const c_char, c_int, ...) -> c_int;

/// `SANDBOX_FILTER_NONE`: the question is whether the process is sandboxed at all.
const FILTER_NONE: c_int = 0;
/// `SANDBOX_FILTER_PATH`, an enum value `libsystem_sandbox` does not export (1, measured).
const FILTER_PATH: c_int = 1;

/// The function's symbol.
pub(super) const CHECK_SYMBOL: &CStr = c"sandbox_check";
/// The flag that keeps a check out of the log: without it every "denied" answer is logged as the
/// process's own denial (about 700 lines a scan, measured), which the watcher would report.
/// Exported as a constant; its value is read through the symbol.
pub(super) const NO_REPORT_SYMBOL: &CStr = c"SANDBOX_CHECK_NO_REPORT";

/// The real kernel's answers, through the looked-up call.
#[derive(Debug)]
pub(crate) struct Seatbelt {
    check: SandboxCheck,
    no_report: c_int,
}

/// The call this process found, looked up once.
static API: OnceLock<Option<Seatbelt>> = OnceLock::new();

/// The call, or [`SweepError::Unavailable`] when this macOS lacks `sandbox_check` or
/// `SANDBOX_CHECK_NO_REPORT`: no scan can run, so no jailed launch may either.
pub(crate) fn api() -> Result<&'static Seatbelt, SweepError> {
    API.get_or_init(|| resolve(lookup))
        .as_ref()
        .ok_or(SweepError::Unavailable)
}

/// `name` in the images this process loaded (`dlsym(RTLD_DEFAULT)`); null when missing.
fn lookup(name: &CStr) -> *mut c_void {
    // SAFETY: RTLD_DEFAULT is a valid pseudo-handle and `name` a NUL-terminated string that
    // outlives the call; dlsym reads nothing else and returns a pointer or null.
    unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }
}

/// The call from `lookup`'s symbols (the real `dlsym`, or a test's), `None` when either is
/// missing.
pub(super) fn resolve(lookup: impl Fn(&CStr) -> *mut c_void) -> Option<Seatbelt> {
    let check = lookup(CHECK_SYMBOL);
    let flag = lookup(NO_REPORT_SYMBOL).cast::<c_int>();
    if check.is_null() || flag.is_null() {
        return None;
    }
    // SAFETY: `check` is the address of libsystem_sandbox's `sandbox_check`, whose C signature
    // `SandboxCheck` spells out (WebKit's and Chromium's declarations agree); a function
    // pointer and a data pointer have the same size on every Apple target.
    let check = unsafe { std::mem::transmute::<*mut c_void, SandboxCheck>(check) };
    // SAFETY: `flag` is the address of an exported, initialised `const int`, never written.
    let no_report = unsafe { flag.read() };
    Some(Seatbelt { check, no_report })
}

impl Checker for Seatbelt {
    fn sandboxed(&self, pid: u32) -> Answer {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return Err(CheckFailed::Failed);
        };
        // SAFETY: a plain pid, a null operation (allowed with FILTER_NONE, which reads no
        // variadic argument) and an integer flag; the call reads nothing else.
        let answer = unsafe { (self.check)(pid, std::ptr::null(), FILTER_NONE | self.no_report) };
        match answer {
            1 => Ok(true),
            0 => Ok(false),
            _ => Err(failure()),
        }
    }

    fn may_write(&self, pid: u32, path: &CStr) -> Answer {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return Err(CheckFailed::Failed);
        };
        // SAFETY: the operation is a NUL-terminated literal; with FILTER_PATH the call reads
        // exactly one variadic argument, a NUL-terminated path, which `path` is and outlives
        // the call.
        let answer = unsafe {
            (self.check)(
                pid,
                c"file-write-data".as_ptr(),
                FILTER_PATH | self.no_report,
                path.as_ptr(),
            )
        };
        match answer {
            0 => Ok(true),
            1 => Ok(false),
            _ => Err(failure()),
        }
    }

    fn still_runs(&self, id: ProcId) -> bool {
        start_of(id.pid()) == Some(id.start)
    }
}

/// Why the call just failed: `ESRCH` is a process that exited since the listing, anything else
/// a call that no longer answers (the scan is then unknown, never "no match").
fn failure() -> CheckFailed {
    if io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
        CheckFailed::Gone
    } else {
        CheckFailed::Failed
    }
}

/// This process's real user id: the agents run as it.
pub(super) fn uid() -> u32 {
    // SAFETY: getuid(2) takes nothing and cannot fail.
    unsafe { libc::getuid() }
}

/// Sets (`on`) or clears the user immutable flag (`UF_IMMUTABLE`) of `path`, never through a
/// link: the mark is immutable while it is in use, so an agent allowed `file-write-data` on it
/// can write no byte to it. Its owner, this process, clears the flag before removing it.
pub(super) fn set_immutable(path: &CStr, on: bool) -> io::Result<()> {
    let flags = if on { libc::UF_IMMUTABLE } else { 0 };
    // SAFETY: `path` is NUL-terminated and outlives the call; lchflags reads nothing else.
    if unsafe { libc::lchflags(path.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
