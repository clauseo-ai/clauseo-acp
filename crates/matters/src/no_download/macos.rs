//! macOS: the thread's I/O policy for dataless files, through `getiopolicy_np` and
//! `setiopolicy_np` (libSystem, macOS 10.5+), which the pinned `libc` does not declare. The
//! values are `<sys/resource.h>`'s (xnu `bsd/sys/resource.h`). The crate's only `unsafe` for
//! the policy is here, behind safe functions.

#![expect(
    unsafe_code,
    reason = "getiopolicy_np and setiopolicy_np, which the pinned libc does not declare"
)]

use std::ffi::c_int;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the refusal to set the policy was logged already: once per process.
static REFUSAL_LOGGED: AtomicBool = AtomicBool::new(false);

/// The policy type: whether reads materialize (download) dataless files.
const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;
/// The policy of the whole process.
#[cfg(test)]
const IOPOL_SCOPE_PROCESS: c_int = 0;
/// The policy of the calling thread, which wins over the process's.
const IOPOL_SCOPE_THREAD: c_int = 1;
/// A thread follows the process policy.
pub(crate) const IOPOL_MATERIALIZE_DATALESS_FILES_DEFAULT: c_int = 0;
/// A read of a dataless file fails with `EDEADLK`.
pub(crate) const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: c_int = 1;
/// A read of a dataless file downloads it.
#[cfg(test)]
pub(crate) const IOPOL_MATERIALIZE_DATALESS_FILES_ON: c_int = 2;

unsafe extern "C" {
    fn getiopolicy_np(iotype: c_int, scope: c_int) -> c_int;
    fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
}

/// The policy at `scope`, or the OS error.
fn get(scope: c_int) -> io::Result<c_int> {
    // SAFETY: plain integers in and out; the call reads only the calling thread or process.
    let policy = unsafe { getiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES, scope) };
    if policy < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(policy)
    }
}

/// The calling thread's policy for dataless files.
pub(crate) fn thread_policy() -> io::Result<c_int> {
    get(IOPOL_SCOPE_THREAD)
}

/// The process policy for dataless files.
#[cfg(test)]
pub(crate) fn process_policy() -> io::Result<c_int> {
    get(IOPOL_SCOPE_PROCESS)
}

/// Sets the calling thread's policy for dataless files. Never the process's.
pub(crate) fn set_thread_policy(policy: c_int) -> io::Result<()> {
    // SAFETY: plain integers in and out; the call changes only the calling thread's policy.
    let done = unsafe {
        setiopolicy_np(
            IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
            IOPOL_SCOPE_THREAD,
            policy,
        )
    };
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Saves the thread's policy and sets it off; returns what to put back, `None` when the set
/// failed (logged once per process: in the App Sandbox it may fail on every guard). A thread
/// whose policy does not read is put back to [`IOPOL_MATERIALIZE_DATALESS_FILES_DEFAULT`], which
/// is what a thread nobody set has.
pub(crate) fn turn_off() -> Option<c_int> {
    let saved = thread_policy().unwrap_or(IOPOL_MATERIALIZE_DATALESS_FILES_DEFAULT);
    match set_thread_policy(IOPOL_MATERIALIZE_DATALESS_FILES_OFF) {
        Ok(()) => Some(saved),
        Err(err) => {
            if !REFUSAL_LOGGED.swap(true, Ordering::Relaxed) {
                tracing::warn!(error = %err, "could not stop reads from downloading cloud-only files");
            }
            None
        }
    }
}

/// Puts back the thread's policy `saved`; a failure is logged.
pub(crate) fn put_back(saved: c_int) {
    if let Err(err) = set_thread_policy(saved) {
        tracing::warn!(error = %err, "could not put back the thread's download policy");
    }
}
