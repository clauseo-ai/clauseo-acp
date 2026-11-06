//! The machine-wide set-up lock (the idea of Codex's `setup_mutex.rs`; no code taken): a named
//! mutex in `Global\`,
//! so two set-ups, extends or removals never run at once in any session. Made with an explicit
//! DACL ([`MUTEX_SDDL`]) giving SYSTEM and Administrators full control and no one else any
//! access, so only an elevated process opens it. Making an object in `Global\` needs
//! `SeCreateGlobalPrivilege`, which a standard user lacks, so no standard user can make the
//! mutex first and hold set-up off.
//!
//! **`WAIT_ABANDONED` counts as acquired.** Release builds use `panic = "abort"`, so a set-up that
//! crashes never runs its guard; Windows then releases the mutex as abandoned when the thread
//! that owned it ends. The set-up that died stopped between two of its steps, and the next run
//! starts from what the record says, so taking the abandoned mutex is safe.
//!
//! A mutex is owned by a thread: [`Held`] is released on the thread that acquired it, so it is
//! not `Send`. The same thread acquiring twice succeeds (Windows mutexes are recursive); the
//! set-up takes the lock once per run.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 mutex and security descriptor calls"
)]

use std::marker::PhantomData;
use std::ptr::null_mut;

use windows::Win32::Foundation::{
    CloseHandle, HANDLE, HLOCAL, LocalFree, WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0,
    WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::core::{HSTRING, PCWSTR};

use super::MachineError;

/// The set-up lock's name.
pub(super) const SETUP_MUTEX: &str = "Global\\ClauseoSandboxSetup";

/// The mutex's security descriptor: a protected DACL, full control (`GA`) for SYSTEM and
/// Administrators only.
pub(super) const MUTEX_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)";

/// How a wait for the lock ended when it got it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Acquired {
    /// No one held it.
    Free,
    /// The set-up that held it ended without releasing it (`WAIT_ABANDONED`).
    Abandoned,
}

/// The set-up lock, opened (made if no process has it open) but not yet held; closed on drop.
#[derive(Debug)]
pub(super) struct SetupMutex(Handle);

impl SetupMutex {
    /// Opens the mutex `name`, making it with [`MUTEX_SDDL`] when it doesn't exist. An existing
    /// mutex keeps its own descriptor (Windows ignores the one passed); an unelevated process
    /// is refused it (`ERROR_ACCESS_DENIED`).
    pub(super) fn open(name: &str) -> Result<SetupMutex, MachineError> {
        let descriptor = Descriptor::from_sddl(MUTEX_SDDL)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: false.into(),
        };
        let name = HSTRING::from(name);
        // SAFETY: `attributes` and the descriptor it points to live for the call; the name is a
        // NUL-terminated HSTRING. The handle returned is closed by `Handle`'s drop.
        let handle = unsafe { CreateMutexW(Some(&raw const attributes), false, &name) }
            .map_err(|error| os("CreateMutexW", &error))?;
        Ok(SetupMutex(Handle(handle)))
    }

    /// Waits up to `wait_ms` milliseconds (0: just tries) for the lock. Held by another thread
    /// or process past that: [`MachineError::LockHeld`]. Abandoned counts as acquired (module
    /// doc). The lock is given back when the [`Held`] drops.
    pub(super) fn acquire(self, wait_ms: u32) -> Result<(Held, Acquired), MachineError> {
        // SAFETY: the handle is a live mutex handle with SYNCHRONIZE access until `self` drops.
        let waited = unsafe { WaitForSingleObject(self.0.0, wait_ms) };
        let acquired = match waited {
            WAIT_OBJECT_0 => Acquired::Free,
            WAIT_ABANDONED => Acquired::Abandoned,
            WAIT_TIMEOUT => return Err(MachineError::LockHeld),
            WAIT_FAILED => {
                let error = windows::core::Error::from_thread();
                return Err(os("WaitForSingleObject", &error));
            }
            other => {
                return Err(MachineError::Other(format!(
                    "WaitForSingleObject gave {}",
                    other.0
                )));
            }
        };
        Ok((
            Held {
                mutex: self,
                thread_bound: PhantomData,
            },
            acquired,
        ))
    }
}

/// The set-up lock, held by this thread; released (`ReleaseMutex`) and closed on drop.
#[derive(Debug)]
pub(super) struct Held {
    mutex: SetupMutex,
    /// A mutex is released by the thread that owns it: `*const ()` keeps `Held` off other
    /// threads.
    thread_bound: PhantomData<*const ()>,
}

impl Held {
    /// Closes the handle without releasing the mutex, as a crashed set-up leaves it: the
    /// mutex is abandoned when this thread ends. For the test of `WAIT_ABANDONED`.
    #[cfg(test)]
    pub(crate) fn abandon(self) {
        let mut held = std::mem::ManuallyDrop::new(self);
        // SAFETY: `held` is never used or dropped again, so the mutex inside is moved out of it
        // exactly once and closed by its own drop.
        drop(unsafe { std::ptr::read(&raw mut held.mutex) });
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: this thread owns the mutex (`acquire` waited on it here, and `Held` is not
        // `Send`). A failed release leaves nothing to do: closing the handle follows, and the
        // mutex is abandoned when the thread ends.
        drop(unsafe { ReleaseMutex(self.mutex.0.0) });
    }
}

/// A kernel handle, closed on drop.
#[derive(Debug)]
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed once, here. A failed close
        // leaves nothing to do.
        drop(unsafe { CloseHandle(self.0) });
    }
}

/// A security descriptor allocated with `LocalAlloc`, freed on drop.
pub(super) struct Descriptor(pub(super) PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// The self-relative descriptor `sddl` describes.
    pub(super) fn from_sddl(sddl: &str) -> Result<Descriptor, MachineError> {
        let text = HSTRING::from(sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
        // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` is a live
        // out-pointer, and the descriptor it receives is freed on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(|error| {
            os(
                "ConvertStringSecurityDescriptorToSecurityDescriptorW",
                &error,
            )
        })?;
        Ok(Descriptor(descriptor))
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: allocated with LocalAlloc by the conversion and freed here, once.
            unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }
}

/// A failed Win32 call as [`MachineError::Os`], with its Win32 code (the HRESULT's low word for
/// a `FACILITY_WIN32` error).
pub(super) fn os(call: &'static str, error: &windows::core::Error) -> MachineError {
    let hresult = error.code().0.cast_unsigned();
    let code = if hresult & 0xFFFF_0000 == 0x8007_0000 {
        hresult & 0xFFFF
    } else {
        hresult
    };
    MachineError::Os { call, code }
}
