//! Windows: the calling thread to `THREAD_PRIORITY_BELOW_NORMAL` and back to
//! `THREAD_PRIORITY_NORMAL`, through a real handle to it (a duplicate of `GetCurrentThread`'s
//! pseudo handle, which names whichever thread uses it), closed after the restore.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 thread priority and handle calls"
)]

use windows::Win32::Foundation::{CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    THREAD_PRIORITY_NORMAL,
};

use super::{Os, Priority};

/// A real handle to a lowered thread, owned until the restore closes it.
pub(crate) struct Lowered(HANDLE);

// SAFETY: a thread handle is a kernel object reference, usable from any thread of the process;
// it is closed once, by the restore that takes it by value.
unsafe impl Send for Lowered {}

impl Priority for Os {
    type Thread = Lowered;

    fn lower_current(&self) -> Option<Lowered> {
        let mut real = HANDLE::default();
        // SAFETY: plain calls with this process's and this thread's pseudo handles; `real` is a
        // valid out pointer for the duplicate.
        let duplicated = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                GetCurrentThread(),
                GetCurrentProcess(),
                &raw mut real,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if duplicated.is_err() {
            return None;
        }
        // SAFETY: `real` is the handle just duplicated, with the access of the pseudo handle.
        if unsafe { SetThreadPriority(real, THREAD_PRIORITY_BELOW_NORMAL) }.is_err() {
            // SAFETY: closed once, here, on the path that keeps no handle.
            drop(unsafe { CloseHandle(real) });
            return None;
        }
        Some(Lowered(real))
    }

    fn restore(&self, thread: Lowered) {
        // SAFETY: `thread.0` is a handle from `lower_current`, still open; a thread that has
        // ended takes the call harmlessly. Closed once, here.
        unsafe {
            drop(SetThreadPriority(thread.0, THREAD_PRIORITY_NORMAL));
            drop(CloseHandle(thread.0));
        }
    }
}
