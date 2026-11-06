//! Windows: reads this process's token and S15's pool record (through `clauseo-sandbox`);
//! `signals.rs` decides. The agent sandbox runs the agent in an `AppContainer`, under a
//! restricted token (restricting SIDs) or as one of its local pool users (ADR 0038 and 0049,
//! tier B).
#![expect(unsafe_code, reason = "FFI to the Win32 token and SID calls")]

#[cfg(test)]
mod tests;

use std::io;

use clauseo_sandbox::windows::RecordError;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::ConvertStringSidToSidW;
use windows::Win32::Security::{
    EqualSid, GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, IsValidSid, PSID,
    TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TOKEN_USER,
    TokenIntegrityLevel, TokenIsAppContainer, TokenRestrictedSids, TokenUser,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HRESULT, HSTRING};

use super::signals::{self, Failure};

// The restricting SIDs' count is read as the buffer's first u32.
const _: () = assert!(std::mem::offset_of!(TOKEN_GROUPS, GroupCount) == 0);

/// `SECURITY_MANDATORY_MEDIUM_RID`: the Medium integrity level's relative id. Pool users run at
/// Low (0x1000).
const MEDIUM_RID: u32 = 0x2000;

/// Whether the process token is an `AppContainer`, has restricting SIDs or belongs to any pool
/// user, or a read was refused (`signals::decide`).
pub(super) fn sandboxed() -> io::Result<bool> {
    signals::decide(Token::open(), sandbox_pool_sids)
}

/// The SIDs of S15's pool users on this machine, from the record S15's set-up writes, read by
/// `clauseo_sandbox::windows::sandbox_pool_sids` beside the code that writes it. Empty when there
/// is no record. A refused read is [`Failure::Denied`], a value that is not a `REG_MULTI_SZ`
/// [`Failure::WrongType`], one that is not UTF-16 or not a list of SIDs [`Failure::NotASid`]:
/// all three count as sandboxed, as #317 counts an unreadable record.
pub(super) fn sandbox_pool_sids() -> Result<Vec<String>, Failure> {
    match clauseo_sandbox::windows::sandbox_pool_sids() {
        Ok(sids) => Ok(sids.into_iter().map(String::from).collect()),
        Err(RecordError::Denied) => Err(Failure::Denied),
        Err(RecordError::WrongType) => Err(Failure::WrongType),
        Err(RecordError::NotText | RecordError::Malformed(_)) => Err(Failure::NotASid),
        Err(RecordError::Other(error)) => Err(Failure::Other(error)),
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

    /// `TokenIntegrityLevel`: a `TOKEN_MANDATORY_LABEL`, whose SID's last sub-authority is the
    /// level. A label that is not a valid SID, or has no sub-authority, is [`Failure::Other`]
    /// (which `signals::decide` counts as sandboxed for this signal).
    fn below_medium(&self) -> Result<bool, Failure> {
        let not_a_label = || Failure::Other(io::Error::other("the integrity label is not a SID"));
        let buffer = self.variable::<TOKEN_MANDATORY_LABEL>(TokenIntegrityLevel)?;
        // SAFETY: GetTokenInformation wrote a TOKEN_MANDATORY_LABEL at the start of `buffer`,
        // which is at least that long and aligned for it (u64 words); its SID points into
        // `buffer`, which lives to the end of this function.
        let label = unsafe { &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
        let sid = label.Label.Sid;
        // SAFETY: `sid` points into `buffer`; IsValidSid only reads it.
        if !unsafe { IsValidSid(sid) }.as_bool() {
            return Err(not_a_label());
        }
        // SAFETY: `sid` is a valid SID (checked above), so its count byte is readable.
        let count = unsafe { *GetSidSubAuthorityCount(sid) };
        let last = count.checked_sub(1).ok_or_else(not_a_label)?;
        // SAFETY: `last` is below the valid SID's sub-authority count.
        let level = unsafe { *GetSidSubAuthority(sid, u32::from(last)) };
        Ok(level < MEDIUM_RID)
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
    /// The SID `sid` spells in the `S-1-…` form; [`Failure::NotASid`] for anything else, an alias
    /// such as `BA` included, or when `ConvertStringSidToSidW` fails.
    fn parse(sid: &str) -> Result<Self, Failure> {
        if !sid.starts_with("S-") {
            return Err(Failure::NotASid);
        }
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
