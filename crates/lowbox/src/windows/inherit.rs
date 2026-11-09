//! The handles a child inherits: exactly the listed ones, through
//! `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, each marked inheritable only for the spawn.
//!
//! `CreateProcess*` with `bInheritHandles` and a handle list passes the listed handles and no
//! other, even when the parent holds more inheritable handles. A listed handle must be
//! inheritable at the call, so [`Marked`] turns the flag on for the caller's handles and puts
//! each one back as it was when dropped, on success and on failure alike.
#![expect(unsafe_code, reason = "FFI to the handle flags and the attribute list")]

use std::ffi::c_void;
use std::os::windows::io::RawHandle;

use ::windows::Win32::Foundation::{
    GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, HANDLE_FLAGS, SetHandleInformation,
};
use ::windows::Win32::System::Threading::{
    DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, UpdateProcThreadAttribute,
};

use super::win32;
use crate::error::Error;
use crate::handles::check_inherit_list;

/// The caller's handles, marked inheritable until dropped.
pub(super) struct Marked {
    /// Each handle and whether it was inheritable before.
    handles: Vec<(HANDLE, bool)>,
}

impl Marked {
    /// Checks `raw` (no null or pseudo handle, none twice) and turns `HANDLE_FLAG_INHERIT` on for
    /// each. A handle that is not open fails the call, and the ones marked so far are put back.
    pub(super) fn new(raw: &[RawHandle]) -> Result<Marked, Error> {
        let values: Vec<usize> = raw.iter().map(|h| h.expose_provenance()).collect();
        check_inherit_list(&values)?;
        let mut marked = Marked {
            handles: Vec::with_capacity(raw.len()),
        };
        for &h in raw {
            let handle = HANDLE(h);
            let mut flags = 0_u32;
            // SAFETY: GetHandleInformation only reads the flags of `handle`, and fails on a value
            // that is no open handle.
            unsafe { GetHandleInformation(handle, &raw mut flags) }
                .map_err(|e| win32("GetHandleInformation", &e))?;
            let was = flags & HANDLE_FLAG_INHERIT.0 != 0;
            if !was {
                // SAFETY: an open handle of this process (read above); the call changes only its
                // inherit flag, which Drop puts back.
                unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) }
                    .map_err(|e| win32("SetHandleInformation", &e))?;
            }
            marked.handles.push((handle, was));
        }
        Ok(marked)
    }

    /// The marked handles, in the caller's order.
    pub(super) fn handles(&self) -> impl Iterator<Item = HANDLE> + '_ {
        self.handles.iter().map(|(h, _)| *h)
    }
}

impl Drop for Marked {
    fn drop(&mut self) {
        for &(handle, was) in &self.handles {
            if !was {
                // SAFETY: the handle was open when marked and the caller still owns it; this puts
                // its inherit flag back off. A failure leaves it inheritable, which only matters
                // to a later spawn without a handle list, so there is nothing to undo here.
                let _left_on: ::windows::core::Result<()> =
                    unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0)) };
            }
        }
    }
}

/// A one-attribute list: `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` over handles that outlive it.
pub(super) struct HandleList {
    /// The list's storage, 8-byte aligned.
    storage: Vec<u64>,
}

impl HandleList {
    /// The list naming `handles`, which must outlive it and stay put.
    pub(super) fn new(handles: &[HANDLE]) -> Result<HandleList, Error> {
        let mut size = 0_usize;
        // SAFETY: the size query: no list, `size` gets the bytes needed. It reports
        // ERROR_INSUFFICIENT_BUFFER by design, so its result is not an error here.
        let _sized: ::windows::core::Result<()> =
            unsafe { InitializeProcThreadAttributeList(None, 1, None, &raw mut size) };
        let mut storage = vec![0_u64; size.div_ceil(8)];
        let raw = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast::<c_void>());
        // SAFETY: `storage` holds at least `size` bytes, 8-byte aligned.
        unsafe { InitializeProcThreadAttributeList(Some(raw), 1, None, &raw mut size) }
            .map_err(|e| win32("InitializeProcThreadAttributeList", &e))?;
        // From here Drop deletes the initialised list, on success and on failure alike.
        let mut list = HandleList { storage };
        // SAFETY: the list is initialised; `handles` outlives it (the caller's contract) and
        // the size is the slice's.
        let updated = unsafe {
            UpdateProcThreadAttribute(
                list.list(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr().cast::<c_void>()),
                size_of_val(handles),
                None,
                None,
            )
        };
        updated.map_err(|e| win32("UpdateProcThreadAttribute", &e))?;
        Ok(list)
    }

    /// The list's pointer.
    pub(super) fn list(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        LPPROC_THREAD_ATTRIBUTE_LIST(self.storage.as_mut_ptr().cast::<c_void>())
    }
}

impl Drop for HandleList {
    fn drop(&mut self) {
        // SAFETY: the list was initialised in `new` (a HandleList exists only after that) and
        // is deleted once, here.
        unsafe { DeleteProcThreadAttributeList(self.list()) };
    }
}
