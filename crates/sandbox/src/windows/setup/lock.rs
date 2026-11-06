//! The machine-wide set-up lock (the idea of Codex's `setup_mutex.rs`; no code taken): a named
//! mutex, so two set-ups, extends or removals never run at once in any session.
//!
//! The mutex lives in a **private namespace** ([`NAMESPACE`]) whose boundary descriptor
//! requires `BUILTIN\Administrators` ([`ADMINISTRATORS`]). Windows lets a process create or open
//! that namespace only when its token holds every SID of the boundary, enabled: an elevated admin
//! or SYSTEM does, a standard user's token (or an admin's filtered one, where Administrators is
//! deny-only) does not. So no standard user can make the name first and hold set-up or
//! `--remove` off, which a plain `Global\` name would allow (`SeCreateGlobalPrivilege` does not
//! gate mutexes in `Global\`). The namespace and the mutex each get an explicit DACL
//! ([`OBJECT_SDDL`]): full control for SYSTEM and Administrators, nothing for anyone else.
//!
//! **`WAIT_ABANDONED` counts as acquired.** Release builds use `panic = "abort"`, so a set-up that
//! crashes never runs its guard; Windows then releases the mutex as abandoned when the thread
//! that owned it ends. The set-up that died stopped between two of its steps, and the next run
//! starts from what the record says, so taking the abandoned mutex is safe.
//!
//! **One namespace handle per process, shared.** A process may have a given private namespace
//! open only once: while one handle lives, `CreatePrivateNamespaceW` finds it and
//! `OpenPrivateNamespaceW` fails with `ERROR_DUP_NAME`. So every [`SetupMutex`] in this process
//! claims the one handle in [`NAMESPACE_HANDLE`] (`shared.rs`): the first claim opens it, the last
//! one dropped closes it, and both happen under the same lock, so no taker opens while a handle
//! that is about to close still lives. A 52 that comes anyway means some other code in this
//! process opened the namespace itself: [`MachineError::Other`], with that said.
//!
//! A mutex is owned by a thread: [`Held`] is released on the thread that acquired it, so it is
//! not `Send`. The same thread acquiring twice succeeds (Windows mutexes are recursive); the
//! set-up takes the lock once per run.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 mutex, private namespace and security descriptor calls"
)]

use std::marker::PhantomData;
use std::ptr::null_mut;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_DUP_NAME, ERROR_NOT_FOUND, HANDLE, HLOCAL, LocalFree,
    WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Threading::{
    AddSIDToBoundaryDescriptor, ClosePrivateNamespace, CreateBoundaryDescriptorW, CreateMutexW,
    CreatePrivateNamespaceW, DeleteBoundaryDescriptor, OpenPrivateNamespaceW, ReleaseMutex,
    WaitForSingleObject,
};
use windows::core::{HRESULT, HSTRING, PCWSTR};

use super::MachineError;
use super::accounts::LocalSid;
use super::shared::{Claim, Shared};
use crate::windows::sids::Sid;

/// The set-up lock's name inside [`NAMESPACE`].
pub(super) const SETUP_MUTEX: &str = "SetupLock";

/// The private namespace's alias prefix; objects in it are named `ClauseoSandbox\<name>`.
pub(super) const NAMESPACE: &str = "ClauseoSandbox";

/// The boundary descriptor's name.
const BOUNDARY: &str = "ClauseoSandboxSetup";

/// `BUILTIN\Administrators`, the boundary's one required SID.
pub(super) const ADMINISTRATORS: &str = "S-1-5-32-544";

/// The namespace's and the mutex's security descriptor: a protected DACL, full control (`GA`)
/// for SYSTEM and Administrators only.
pub(super) const OBJECT_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)";

/// How many times opening the namespace is tried when it vanishes between a create that found
/// it and the open (its last holder closed it in between).
const NAMESPACE_TRIES: usize = 3;

/// This process's one handle to [`NAMESPACE`] (module doc).
static NAMESPACE_HANDLE: Shared<Namespace> = Shared::new();

/// How a wait for the lock ended when it got it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Acquired {
    /// No one held it.
    Free,
    /// The set-up that held it ended without releasing it (`WAIT_ABANDONED`).
    Abandoned,
}

/// The set-up lock, opened (made if no process has it open) but not yet held. Dropping it closes
/// the mutex, then gives back its claim on the namespace (fields drop in order), which closes
/// it when no other `SetupMutex` in this process holds it.
#[derive(Debug)]
pub(super) struct SetupMutex {
    mutex: Handle,
    _namespace: Claim<Namespace>,
}

impl SetupMutex {
    /// Opens the mutex `name` in [`NAMESPACE`], making the namespace and the mutex with
    /// [`OBJECT_SDDL`] when they don't exist. An existing mutex keeps its own descriptor (Windows
    /// ignores the one passed). A token without Administrators enabled is refused the namespace.
    pub(super) fn open(name: &str) -> Result<SetupMutex, MachineError> {
        let namespace = NAMESPACE_HANDLE.claim(Namespace::open, || {
            MachineError::Other("too many holders of the set-up lock's namespace".to_owned())
        })?;
        let descriptor = Descriptor::from_sddl(OBJECT_SDDL)?;
        let attributes = attributes(&descriptor);
        let name = HSTRING::from(format!("{NAMESPACE}\\{name}"));
        // SAFETY: `attributes` and the descriptor it points to live for the call; the name is a
        // NUL-terminated HSTRING. The handle returned is closed by `Handle`'s drop.
        let handle = unsafe { CreateMutexW(Some(&raw const attributes), false, &name) }
            .map_err(|error| os("CreateMutexW", &error))?;
        Ok(SetupMutex {
            mutex: Handle(handle),
            _namespace: namespace,
        })
    }

    /// Waits up to `wait_ms` milliseconds (0: just tries) for the lock. Held by another thread
    /// or process past that: [`MachineError::LockHeld`]. Abandoned counts as acquired (module
    /// doc). The lock is given back when the [`Held`] drops.
    pub(super) fn acquire(self, wait_ms: u32) -> Result<(Held, Acquired), MachineError> {
        // SAFETY: the handle is a live mutex handle with SYNCHRONIZE access until `self` drops.
        let waited = unsafe { WaitForSingleObject(self.mutex.0, wait_ms) };
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
        drop(unsafe { ReleaseMutex(self.mutex.mutex.0) });
    }
}

/// `descriptor` as non-inheritable security attributes.
fn attributes(descriptor: &Descriptor) -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    }
}

/// The set-up's private namespace, opened; closed (`ClosePrivateNamespace`, not destroyed) on
/// drop, so it lasts while any process has it or an object in it open.
#[derive(Debug)]
struct Namespace(HANDLE);

// SAFETY: a private-namespace handle is a process-wide kernel handle that any thread may use or
// close. It is only closed in `Drop`, and `NAMESPACE_HANDLE` drops it once, under its lock,
// when its last claim goes.
unsafe impl Send for Namespace {}

impl Namespace {
    /// Creates [`NAMESPACE`] bounded by Administrators, or opens it when it exists.
    fn open() -> Result<Namespace, MachineError> {
        let boundary = Boundary::new()?;
        let descriptor = Descriptor::from_sddl(OBJECT_SDDL)?;
        let attributes = attributes(&descriptor);
        let alias = HSTRING::from(NAMESPACE);
        for _try in 0..NAMESPACE_TRIES {
            // SAFETY: `attributes`, its descriptor and the boundary live for the call; the alias
            // is NUL-terminated. A handle returned is closed by `Namespace`'s drop.
            let made = unsafe {
                CreatePrivateNamespaceW(Some(&raw const attributes), boundary.0.0, &alias)
            };
            if !made.is_invalid() {
                return Ok(Namespace(made));
            }
            let error = windows::core::Error::from_thread();
            if error.code() != HRESULT::from_win32(ERROR_ALREADY_EXISTS.0) {
                return Err(os("CreatePrivateNamespaceW", &error));
            }
            // SAFETY: the boundary lives for the call; the alias is NUL-terminated. A handle
            // returned is closed by `Namespace`'s drop.
            let opened = unsafe { OpenPrivateNamespaceW(boundary.0.0, &alias) };
            if !opened.is_invalid() {
                return Ok(Namespace(opened));
            }
            let error = windows::core::Error::from_thread();
            if error.code() == HRESULT::from_win32(ERROR_DUP_NAME.0) {
                return Err(MachineError::Other(
                    "other code in this process holds the set-up lock's namespace open".to_owned(),
                ));
            }
            if error.code() != HRESULT::from_win32(ERROR_NOT_FOUND.0) {
                return Err(os("OpenPrivateNamespaceW", &error));
            }
        }
        Err(MachineError::Other(
            "the set-up lock's namespace kept vanishing while it was opened".to_owned(),
        ))
    }
}

impl Drop for Namespace {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreatePrivateNamespaceW or OpenPrivateNamespaceW and is
        // closed once, here; flags 0 closes without destroying it for other holders. A failed
        // close leaves nothing to do.
        let _closed = unsafe { ClosePrivateNamespace(self.0, 0) };
    }
}

/// A boundary descriptor requiring [`ADMINISTRATORS`]; deleted on drop.
struct Boundary(HANDLE);

impl Boundary {
    fn new() -> Result<Boundary, MachineError> {
        let name = HSTRING::from(BOUNDARY);
        // SAFETY: the name is NUL-terminated and lives for the call. The descriptor returned
        // is deleted by `Boundary`'s drop.
        let made = unsafe { CreateBoundaryDescriptorW(&name, 0) };
        if made.is_invalid() {
            return Err(os(
                "CreateBoundaryDescriptorW",
                &windows::core::Error::from_thread(),
            ));
        }
        let mut boundary = Boundary(made);
        let admins =
            Sid::parse(ADMINISTRATORS).map_err(|error| MachineError::Other(error.to_string()))?;
        let admins = LocalSid::parse(&admins)?;
        // SAFETY: `boundary.0` is a live boundary descriptor, which the call may reallocate
        // and write back; the SID is valid for the call.
        unsafe { AddSIDToBoundaryDescriptor(&raw mut boundary.0, admins.0) }
            .map_err(|error| os("AddSIDToBoundaryDescriptor", &error))?;
        Ok(boundary)
    }
}

impl Drop for Boundary {
    fn drop(&mut self) {
        // SAFETY: the descriptor came from CreateBoundaryDescriptorW (and perhaps
        // AddSIDToBoundaryDescriptor's reallocation) and is deleted once, here.
        unsafe { DeleteBoundaryDescriptor(self.0) };
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
