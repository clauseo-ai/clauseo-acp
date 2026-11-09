//! The Windows side: the owner's namespace, the guard's handle checks and the `LowBox` launch.
//! The native calls the `windows` crate does not declare are declared in `nt`.

mod namespace;
mod nt;
mod probe;
mod spawn;
mod stdio;
#[cfg(test)]
mod tests;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use ::windows::Win32::Foundation::NTSTATUS;

pub use namespace::{Namespace, NamespaceHandles};
pub use spawn::{LowBoxCommand, Spawned};
pub use stdio::Stdio;

use crate::error::Error;

/// `text` as a NUL-terminated UTF-16 string.
fn wide_nul(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

/// The error for a failed Win32 `call`, from the thread's last error.
fn last_error(call: &'static str) -> Error {
    win32(call, &::windows::core::Error::from_thread())
}

/// The error for a failed Win32 `call` that returned a `windows::core::Error`.
fn win32(call: &'static str, error: &::windows::core::Error) -> Error {
    // An HRESULT from a Win32 code is 0x8007xxxx; the low 16 bits are the code.
    let hresult = error.code().0.cast_unsigned();
    let code = if hresult & 0xFFFF_0000 == 0x8007_0000 {
        hresult & 0xFFFF
    } else {
        hresult
    };
    Error::Win32 { call, code }
}

/// `Ok` for a success NTSTATUS, else the error for `call`.
fn nt_ok(call: &'static str, status: NTSTATUS) -> Result<(), Error> {
    if status.0 >= 0 {
        Ok(())
    } else {
        Err(Error::Nt {
            call,
            status: status.0,
        })
    }
}
