//! Windows: reads this process's token and S15's sandbox user record; `signals.rs` decides. The
//! agent sandbox runs the agent in an `AppContainer`, under a restricted token (restricting
//! SIDs) or as its own local sandbox user (ADR 0038, tier B).
#![expect(unsafe_code, reason = "FFI to the Win32 token, SID and registry calls")]

#[cfg(test)]
mod tests;

use std::io;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER,
    ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, ERROR_UNSUPPORTED_TYPE, HANDLE, HLOCAL, LocalFree,
    WIN32_ERROR,
};
use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows::Win32::Security::{
    EqualSid, GetTokenInformation, PSID, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_QUERY,
    TOKEN_USER, TokenIsAppContainer, TokenRestrictedSids, TokenUser,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HRESULT, HSTRING, PCWSTR, w};

use super::signals::{self, Failure};

// The restricting SIDs' count is read as the buffer's first u32.
const _: () = assert!(std::mem::offset_of!(TOKEN_GROUPS, GroupCount) == 0);

/// Whether the process token is an `AppContainer`, has restricting SIDs or belongs to the
/// sandbox user, or a read was refused (`signals::decide`).
pub(super) fn sandboxed() -> io::Result<bool> {
    signals::decide(Token::open(), sandbox_user_sid)
}

/// The SID of S15's sandbox user on this machine, as S15's elevated set-up records it:
/// `HKLM\SOFTWARE\Clauseo\Sandbox`, value `UserSid`, a `REG_SZ`, in the 64-bit view, readable by
/// everyone. `Ok(None)` when the key or the value is missing (no sandbox user here). The only
/// place that names the record: when S15 lands it moves into `clauseo-sandbox`
/// (`windows::sandbox_user_sid()`), beside the code that writes it.
pub(super) fn sandbox_user_sid() -> Result<Option<String>, Failure> {
    read_string(w!("SOFTWARE\\Clauseo\\Sandbox"), w!("UserSid"))
}

/// The `REG_SZ` `value` of `HKLM\<subkey>`, read in the 64-bit view (`RRF_SUBKEY_WOW6464KEY`,
/// `RegGetValueW`'s `KEY_WOW64_64KEY`). Missing key or value: `Ok(None)`. Another type:
/// [`Failure::WrongType`]. Refused: [`Failure::Denied`]. Not UTF-16: [`Failure::NotASid`].
fn read_string(subkey: PCWSTR, value: PCWSTR) -> Result<Option<String>, Failure> {
    let flags = RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY;
    let mut size = 0u32;
    // SAFETY: the key and value names are NUL-terminated literals; no data pointer asks only for
    // the size, written to `size`.
    let found = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            flags,
            None,
            None,
            Some(&raw mut size),
        )
    };
    if !present(found)? {
        return Ok(None);
    }
    let units = usize::try_from(size)
        .map_err(|error| Failure::Other(io::Error::other(error)))?
        .div_ceil(2)
        .max(1);
    let mut buffer = vec![0u16; units];
    let mut written = u32::try_from(size_of_val(buffer.as_slice()))
        .map_err(|error| Failure::Other(io::Error::other(error)))?;
    // SAFETY: `buffer` is live, writable and `written` bytes long for the call.
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            subkey,
            value,
            flags,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut written),
        )
    };
    if !present(read)? {
        return Ok(None);
    }
    let units = usize::try_from(written / 2).map_or(0, |n| n.min(buffer.len()));
    let text = buffer.get(..units).unwrap_or_default();
    let text = text.split(|unit| *unit == 0).next().unwrap_or_default();
    String::from_utf16(text)
        .map(Some)
        .map_err(|_not_utf16| Failure::NotASid)
}

/// What a `RegGetValueW` result means: `Ok(true)` read, `Ok(false)` missing. Every code but
/// these five is [`Failure::Other`] (for example `ERROR_MORE_DATA` when the value grew between
/// the two reads, or `ERROR_NOT_ENOUGH_MEMORY`).
fn present(code: WIN32_ERROR) -> Result<bool, Failure> {
    match code {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(false),
        ERROR_ACCESS_DENIED => Err(Failure::Denied),
        ERROR_UNSUPPORTED_TYPE => Err(Failure::WrongType),
        other => Err(Failure::Other(io::Error::from_raw_os_error(
            i32::try_from(other.0).unwrap_or(i32::MAX),
        ))),
    }
}

/// A Win32 error as a [`Failure`]: [`Failure::Denied`] for `ERROR_ACCESS_DENIED`.
fn failure(error: windows::core::Error) -> Failure {
    if error.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        Failure::Denied
    } else {
        Failure::Other(error.into())
    }
}

/// This process's token, opened to query; closed on drop.
struct Token(HANDLE);

impl Token {
    fn open() -> Result<Self, Failure> {
        let mut handle = HANDLE::default();
        // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `handle` is a
        // live out-pointer for the call.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut handle) }
            .map_err(failure)?;
        Ok(Self(handle))
    }

    /// The token's `class` information, in u64 words: the structures hold pointers, so the
    /// buffer is aligned for them. At least `size_of::<T>()` bytes long.
    fn variable<T>(&self, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<u64>, Failure> {
        let mut needed = 0u32;
        // SAFETY: no buffer and a zero length ask only for the size, written to `needed`.
        let sized = unsafe { GetTokenInformation(self.0, class, None, 0, &raw mut needed) };
        match sized {
            Err(error) if error.code() == HRESULT::from_win32(ERROR_INSUFFICIENT_BUFFER.0) => {}
            Err(error) => return Err(failure(error)),
            Ok(()) => return Err(Failure::Other(io::Error::other("the token gave no size"))),
        }
        let bytes = usize::try_from(needed)
            .map_err(|error| Failure::Other(io::Error::other(error)))?
            .max(size_of::<T>());
        let mut buffer = vec![0u64; bytes.div_ceil(size_of::<u64>())];
        self.information(class, &mut buffer)?;
        Ok(buffer)
    }

    /// Fills `buffer` with the token's `class` information.
    fn information<T: Copy>(
        &self,
        class: TOKEN_INFORMATION_CLASS,
        buffer: &mut [T],
    ) -> Result<(), Failure> {
        let len = u32::try_from(size_of_val(buffer))
            .map_err(|error| Failure::Other(io::Error::other(error)))?;
        let mut written = 0u32;
        // SAFETY: `buffer` is live, writable and `len` bytes long for the call.
        unsafe {
            GetTokenInformation(
                self.0,
                class,
                Some(buffer.as_mut_ptr().cast()),
                len,
                &raw mut written,
            )
        }
        .map_err(failure)
    }
}

impl signals::Token for Token {
    /// `TokenIsAppContainer`: a DWORD, non-zero for an `AppContainer` token.
    fn app_container(&self) -> Result<bool, Failure> {
        let mut answer = [0u32; 1];
        self.information(TokenIsAppContainer, &mut answer)?;
        Ok(answer[0] != 0)
    }

    /// `TokenRestrictedSids`: a `TOKEN_GROUPS`; restricted when its count is not zero.
    fn restricted(&self) -> Result<bool, Failure> {
        let buffer = self.variable::<TOKEN_GROUPS>(TokenRestrictedSids)?;
        // GroupCount is TOKEN_GROUPS's first field (asserted above), and the buffer holds at least
        // one u64, so its first 4 bytes in memory order are the count.
        let first = buffer.first().copied().unwrap_or_default().to_ne_bytes();
        let count = u32::from_ne_bytes([first[0], first[1], first[2], first[3]]);
        Ok(count != 0)
    }

    /// `TokenUser`'s SID against the one `sid` spells, with `EqualSid`.
    fn user_is(&self, sid: &str) -> Result<bool, Failure> {
        let wanted = LocalSid::parse(sid)?;
        let buffer = self.variable::<TOKEN_USER>(TokenUser)?;
        // SAFETY: GetTokenInformation wrote a TOKEN_USER at the start of `buffer`, which is at
        // least that long and aligned for it (u64 words); its SID points into `buffer`, which
        // lives to the end of this function.
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        // SAFETY: both are valid SIDs that live for the call. EqualSid fails only on an invalid
        // SID, and its FALSE for "not equal" comes back as an error too; either way not equal.
        Ok(unsafe { EqualSid(user.User.Sid, wanted.0) }.is_ok())
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcessToken and is closed once, here. A failed close
        // leaves nothing to do.
        drop(unsafe { CloseHandle(self.0) });
    }
}

/// A SID that `ConvertStringSidToSidW` allocated; freed with `LocalFree` on drop.
struct LocalSid(PSID);

impl LocalSid {
    /// The SID `sid` spells; [`Failure::NotASid`] when `ConvertStringSidToSidW` fails.
    fn parse(sid: &str) -> Result<Self, Failure> {
        let mut parsed = PSID::default();
        // SAFETY: the HSTRING is NUL-terminated and lives for the call; `parsed` is a live
        // out-pointer.
        unsafe { ConvertStringSidToSidW(&HSTRING::from(sid), &raw mut parsed) }
            .map_err(|_not_a_sid| Failure::NotASid)?;
        Ok(Self(parsed))
    }
}

impl Drop for LocalSid {
    fn drop(&mut self) {
        // SAFETY: ConvertStringSidToSidW allocated the SID with LocalAlloc; it is freed once,
        // here. A failed free leaves nothing to do, so its answer is not looked at.
        unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
    }
}
