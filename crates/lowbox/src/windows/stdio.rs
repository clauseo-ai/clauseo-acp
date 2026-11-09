//! What the child's standard handles are, and the handles that make them.
#![expect(
    unsafe_code,
    reason = "FFI to GetStdHandle, DuplicateHandle, CreateFileW and CreatePipe"
)]

use std::fs::File;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use ::windows::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, GENERIC_READ, GENERIC_WRITE, HANDLE,
    HANDLE_FLAG_INHERIT, SetHandleInformation,
};
use ::windows::Win32::Security::SECURITY_ATTRIBUTES;
use ::windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use ::windows::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use ::windows::Win32::System::Pipes::CreatePipe;
use ::windows::Win32::System::Threading::GetCurrentProcess;
use ::windows::core::w;

use super::win32;
use crate::error::Error;

/// What one of the child's standard handles is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stdio {
    /// The guard's own handle (`NUL` when the guard has none).
    #[default]
    Inherit,
    /// `NUL`.
    Null,
    /// A new anonymous pipe; the guard keeps the other end ([`crate::Spawned`]).
    Piped,
}

/// Which standard handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Which {
    /// Standard input: the child reads.
    Input,
    /// Standard output.
    Output,
    /// Standard error.
    Error,
}

/// The child's end, inheritable, and the guard's end of a pipe.
pub(super) struct Ends {
    /// The handle the child gets, inheritable.
    pub(super) child: OwnedHandle,
    /// The guard's end of a pipe.
    pub(super) parent: Option<File>,
}

impl Stdio {
    /// The handles for `which`.
    pub(super) fn ends(self, which: Which) -> Result<Ends, Error> {
        match self {
            Stdio::Inherit => {
                let child = own_std(which)?.map_or_else(null, Ok)?;
                Ok(Ends {
                    child,
                    parent: None,
                })
            }
            Stdio::Null => Ok(Ends {
                child: null()?,
                parent: None,
            }),
            Stdio::Piped => pipe(which),
        }
    }
}

/// An inheritable copy of the guard's standard handle `which`, or `None` when it has none.
fn own_std(which: Which) -> Result<Option<OwnedHandle>, Error> {
    let id: STD_HANDLE = match which {
        Which::Input => STD_INPUT_HANDLE,
        Which::Output => STD_OUTPUT_HANDLE,
        Which::Error => STD_ERROR_HANDLE,
    };
    // SAFETY: GetStdHandle only reads this process's parameters.
    let Ok(std) = (unsafe { GetStdHandle(id) }) else {
        return Ok(None);
    };
    if std.is_invalid() {
        return Ok(None);
    }
    let mut copy = HANDLE::default();
    // SAFETY: `std` is this process's standard handle; the copy is a new handle in this
    // process, owned below.
    unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            std,
            GetCurrentProcess(),
            &raw mut copy,
            0,
            true,
            DUPLICATE_SAME_ACCESS,
        )
    }
    .map_err(|e| win32("DuplicateHandle", &e))?;
    Ok(Some(owned(copy)))
}

/// `NUL`, opened for reading and writing, inheritable.
fn null() -> Result<OwnedHandle, Error> {
    let inherit = inheritable();
    // SAFETY: a constant NUL-terminated name and a SECURITY_ATTRIBUTES that outlives the call.
    let handle = unsafe {
        CreateFileW(
            w!("NUL"),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            Some(&raw const inherit),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .map_err(|e| win32("CreateFileW", &e))?;
    Ok(owned(handle))
}

/// A pipe whose child end is inheritable: the read end for input, the write end otherwise.
fn pipe(which: Which) -> Result<Ends, Error> {
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    // SAFETY: both out pointers are valid; no attributes, so neither end is inheritable yet.
    unsafe { CreatePipe(&raw mut read, &raw mut write, None, 0) }
        .map_err(|e| win32("CreatePipe", &e))?;
    let (read, write) = (owned(read), owned(write));
    let (child, parent) = match which {
        Which::Input => (read, write),
        Which::Output | Which::Error => (write, read),
    };
    // SAFETY: `child` is an open pipe end of this process; the call only sets its flag.
    unsafe {
        SetHandleInformation(
            HANDLE(child.as_raw_handle()),
            HANDLE_FLAG_INHERIT.0,
            HANDLE_FLAG_INHERIT,
        )
    }
    .map_err(|e| win32("SetHandleInformation", &e))?;
    Ok(Ends {
        child,
        parent: Some(File::from(parent)),
    })
}

/// Attributes that make a new handle inheritable.
fn inheritable() -> SECURITY_ATTRIBUTES {
    SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
        lpSecurityDescriptor: std::ptr::null_mut(),
        bInheritHandle: true.into(),
    }
}

/// Takes ownership of a handle a call just returned.
fn owned(handle: HANDLE) -> OwnedHandle {
    // SAFETY: the call succeeded, so `handle` is open, and nothing else holds it.
    unsafe { OwnedHandle::from_raw_handle(handle.0) }
}
