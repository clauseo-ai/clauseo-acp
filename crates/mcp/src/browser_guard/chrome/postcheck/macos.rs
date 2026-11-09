//! macOS: Chrome's children through libproc (`proc_listchildpids`, walked down from Chrome),
//! each one's start time and state through `proc_pidinfo(PROC_PIDTBSDINFO)` (a zombie is gone),
//! their arguments through `sysctl KERN_PROCARGS2` (`record/scan/macos.rs`), and what Chrome's
//! and each child's Seatbelt profile allows through `sandbox_check` (`libsystem_sandbox`, part
//! of libSystem; declared in `<sandbox.h>`, undocumented), judged in `judge.rs`. The question is
//! `file-read-data` of Chrome's `Default/Preferences` in its profile folder, with the profile's
//! real path (Seatbelt matches resolved paths: `/var` is `/private/var`). `sandbox_check` is
//! looked up at run time (`dlsym`), so a macOS without it refuses the browser
//! ([`Refused::NoSandboxCheck`]) instead of failing to load the guard; so is
//! `SANDBOX_CHECK_NO_REPORT`, which keeps each question out of the system log when present.
//! Each call has one wrapper here, with its `unsafe` block and why it is sound. Compiled for
//! macOS, never run there yet (decision record 0029, "Not verified").
#![expect(
    unsafe_code,
    reason = "FFI to libproc (proc_listchildpids, proc_pidinfo) and, through dlsym, to \
              libsystem_sandbox (sandbox_check)"
)]

use std::ffi::{CString, c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use nix::libc;

use super::super::super::record::scan::macos::Reader;
use super::super::super::session::Refused;
use super::Source;
use super::judge::{self, Mode};

/// `SANDBOX_FILTER_PATH` of `enum sandbox_filter_type`: one `const char *` path follows.
const SANDBOX_FILTER_PATH: c_int = 1;
/// The operation asked about.
const OPERATION: &std::ffi::CStr = c"file-read-data";
/// `sandbox_check`'s type: `int sandbox_check(pid_t, const char *operation, int type, ...)`.
type SandboxCheck = unsafe extern "C" fn(c_int, *const c_char, c_int, ...) -> c_int;
/// How many child pids the first `proc_listchildpids` call makes room for; a full buffer is
/// retried with twice the room, up to [`MAX_CHILDREN`].
const FIRST_CHILDREN: usize = 64;
/// The most children of one process read; more fails the listing.
const MAX_CHILDREN: usize = 65_536;
/// The most processes below Chrome walked; more fails the listing.
const MAX_BELOW: usize = 4_096;

/// Reads libproc afresh at each call; the argument reader is made at the first.
pub(super) struct Libproc {
    reader: Option<Reader>,
    check: SandboxCheck,
    /// `SANDBOX_FILTER_PATH`, with `SANDBOX_CHECK_NO_REPORT` when the system has it.
    filter: c_int,
    /// `<profile>/Default/Preferences`, the profile's real path; `None` when the profile's
    /// path cannot be resolved, which refuses every child.
    path: Option<CString>,
    mode: Mode,
}

impl Libproc {
    /// The source for a Chrome whose profile is `profile`, judged in `mode`;
    /// [`Refused::NoSandboxCheck`] when this system has no `sandbox_check`.
    #[expect(
        clippy::disallowed_methods,
        reason = "macOS only: canonicalize's verbatim prefix is a Windows matter, and Seatbelt \
                  matches the resolved path"
    )]
    pub(super) fn new(profile: &Path, mode: Mode) -> Result<Libproc, Refused> {
        let check = found(symbol(c"sandbox_check"))?;
        let no_report = symbol(c"SANDBOX_CHECK_NO_REPORT").map_or(0, |at| {
            // SAFETY: the symbol is libsystem_sandbox's exported `const enum
            // sandbox_filter_type SANDBOX_CHECK_NO_REPORT`, an int-sized value, readable for the
            // life of the process (the library is part of libSystem and never unloaded).
            unsafe { at.cast::<c_int>().read() }
        });
        let path = std::fs::canonicalize(profile)
            .ok()
            .map(|real| real.join("Default").join("Preferences"))
            .and_then(|path| CString::new(path.as_os_str().as_bytes()).ok());
        Ok(Libproc {
            reader: None,
            check,
            filter: SANDBOX_FILTER_PATH | no_report,
            path,
            mode,
        })
    }

    /// `sandbox_check`'s answer for `pid`: 0 allowed, 1 denied, -1 not answered.
    fn ask(&self, pid: u32) -> i32 {
        let (Ok(pid), Some(path)) = (c_int::try_from(pid), self.path.as_ref()) else {
            return -1;
        };
        // SAFETY: `self.check` is libsystem_sandbox's sandbox_check (found by name, with the
        // type its header declares); the operation and the path are live NUL-terminated
        // strings, and SANDBOX_FILTER_PATH takes exactly one `const char *` variadic argument,
        // which is passed. A pid that names no process gets -1.
        unsafe { (self.check)(pid, OPERATION.as_ptr(), self.filter, path.as_ptr()) }
    }
}

/// The address of `name` in the libraries loaded, or `None`.
fn symbol(name: &std::ffi::CStr) -> Option<*mut c_void> {
    // SAFETY: RTLD_DEFAULT searches every loaded image, and `name` is a live NUL-terminated
    // string; dlsym returns null when nothing has the name.
    let at = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    (!at.is_null()).then_some(at)
}

/// `sandbox_check` from its address, or the refusal when the system has none.
pub(super) fn found(at: Option<*mut c_void>) -> Result<SandboxCheck, Refused> {
    let at = at.ok_or(Refused::NoSandboxCheck)?;
    // SAFETY: `at` is the non-null address of the function libsystem_sandbox exports as
    // `sandbox_check`, whose C declaration is the `SandboxCheck` type.
    Ok(unsafe { std::mem::transmute::<*mut c_void, SandboxCheck>(at) })
}

impl Source for Libproc {
    fn below(&mut self, chrome: u32) -> Option<Vec<u32>> {
        let mut found = Vec::new();
        let mut next = vec![chrome];
        while let Some(pid) = next.pop() {
            for child in children(pid)? {
                if child != chrome && !found.contains(&child) {
                    found.push(child);
                    next.push(child);
                }
            }
            if found.len() > MAX_BELOW {
                return None;
            }
        }
        // A zombie has exited: not a child to judge.
        found.retain(|&pid| bsd_info(pid).is_none_or(|info| info.pbi_status != libc::SZOMB));
        Some(found)
    }

    fn stamp(&mut self, pid: u32) -> Option<u64> {
        let info = bsd_info(pid)?;
        (info.pbi_status != libc::SZOMB)
            .then(|| info.pbi_start_tvsec.saturating_mul(1_000_000) + info.pbi_start_tvusec)
    }

    fn args(&mut self, pid: u32) -> Option<Vec<u8>> {
        if self.reader.is_none() {
            self.reader = Reader::new();
        }
        self.reader.as_mut()?.args_of(i32::try_from(pid).ok()?)
    }

    fn sandboxed(&mut self, chrome: u32, pid: u32) -> Result<(), &'static str> {
        judge::macos(self.mode, self.ask(chrome), self.ask(pid))
    }
}

/// `pid`'s BSD information; `None` when it cannot be read (the process has gone).
fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    let pid = c_int::try_from(pid).ok()?;
    let size = c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: proc_bsdinfo is plain integers and byte arrays, for which all zeroes is valid.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a live proc_bsdinfo of exactly `size` bytes, the struct this flavour
    // writes, and the kernel writes no more than `size`.
    let written =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size) };
    (written == size).then_some(info)
}

/// The pids whose parent is `parent`; `None` when the call answers -1 (its failure), for a pid
/// out of range, or past [`MAX_CHILDREN`]. A count of 0 reads as no children (xnu's
/// `proc_listpids` answers a failed system call with 0), which the check reads as no renderer
/// yet, never as a pass.
fn children(parent: u32) -> Option<Vec<u32>> {
    let parent = c_int::try_from(parent).ok()?;
    let mut room = FIRST_CHILDREN;
    loop {
        let mut pids: Vec<c_int> = vec![0; room];
        let bytes = c_int::try_from(pids.len() * size_of::<c_int>()).ok()?;
        // SAFETY: `pids` is a live, zeroed buffer of exactly `bytes` bytes that this call may
        // fill with pids and nothing else; the kernel writes at most `bytes` bytes into it.
        // The return counts pids (xnu libsyscall/wrappers/libproc/libproc.c divides the byte
        // count by `sizeof(int)`) or is -1 on failure, which `usize::try_from` below refuses; a
        // count is used only capped at the buffer's length, so no read passes it.
        let count = unsafe { libc::proc_listchildpids(parent, pids.as_mut_ptr().cast(), bytes) };
        let count = usize::try_from(count).ok()?;
        if count < room {
            pids.truncate(count);
            // Unused slots stay 0, which is never a child's pid.
            return Some(
                pids.into_iter()
                    .filter_map(|pid| u32::try_from(pid).ok().filter(|&pid| pid != 0))
                    .collect(),
            );
        }
        if room >= MAX_CHILDREN {
            return None;
        }
        room *= 2;
    }
}
