//! macOS: Chrome's children through libproc (`proc_listchildpids`, walked down from Chrome),
//! their arguments through `sysctl KERN_PROCARGS2` (`record/scan/macos.rs`), and whether each
//! runs under a Seatbelt profile through `sandbox_check` (`libsystem_sandbox`, part of libSystem;
//! declared in `<sandbox.h>`, undocumented; `crates/desktop/src/jailed/macos.rs` asks it about
//! its own process the same way). Each call has one wrapper here, with its `unsafe` block and
//! why it is sound. Compiled for macOS, never run there yet (decision record 0029, "Not
//! verified").
#![expect(
    unsafe_code,
    reason = "FFI to libproc (proc_listchildpids) and libsystem_sandbox (sandbox_check)"
)]

use std::ffi::{c_char, c_int};

use nix::libc;

use super::super::super::record::scan::macos::Reader;
use super::{Source, judge};

/// `SANDBOX_FILTER_NONE` of `enum sandbox_filter_type`: no filter argument follows.
const SANDBOX_FILTER_NONE: c_int = 0;
/// How many child pids the first `proc_listchildpids` call makes room for; a full buffer is
/// retried with twice the room, up to [`MAX_CHILDREN`].
const FIRST_CHILDREN: usize = 64;
/// The most children of one process read; more fails the listing.
const MAX_CHILDREN: usize = 65_536;
/// The most processes below Chrome walked; more fails the listing.
const MAX_BELOW: usize = 4_096;

unsafe extern "C" {
    fn sandbox_check(pid: c_int, operation: *const c_char, filter: c_int, ...) -> c_int;
}

/// Reads libproc afresh at each call; the argument reader is made at the first.
#[derive(Default)]
pub(super) struct Libproc {
    reader: Option<Reader>,
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
        Some(found)
    }

    fn args(&mut self, pid: u32) -> Option<Vec<u8>> {
        if self.reader.is_none() {
            self.reader = Reader::new();
        }
        self.reader.as_mut()?.args_of(i32::try_from(pid).ok()?)
    }

    fn sandboxed(&mut self, _chrome: u32, pid: u32) -> Result<(), &'static str> {
        let Ok(pid) = c_int::try_from(pid) else {
            return judge::macos(-1);
        };
        // SAFETY: sandbox_check reads no pointer when the operation is null and the filter is
        // SANDBOX_FILTER_NONE, which takes no variadic argument; `pid` is a plain integer, and a
        // pid that names no process gets -1.
        let answer = unsafe { sandbox_check(pid, std::ptr::null(), SANDBOX_FILTER_NONE) };
        judge::macos(answer)
    }
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
