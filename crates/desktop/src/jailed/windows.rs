//! Windows: reads this process's token. The agent sandbox runs the agent under a restricted
//! token (restricting SIDs) or in an `AppContainer`; either one means a jailed start.
#![expect(unsafe_code, reason = "FFI to the Win32 token calls")]

use std::io;

use windows::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, HANDLE};
use windows::Win32::Security::{
    GetTokenInformation, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TokenIsAppContainer,
    TokenRestrictedSids,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::HRESULT;

// The restricting SIDs' count is read as the buffer's first u32.
const _: () = assert!(std::mem::offset_of!(TOKEN_GROUPS, GroupCount) == 0);

/// Whether the process token is an `AppContainer` or has restricting SIDs.
pub(super) fn sandboxed() -> io::Result<bool> {
    let token = Token::open()?;
    Ok(token.app_container()? || token.restricted()?)
}

/// This process's token, opened to query; closed on drop.
struct Token(HANDLE);

impl Token {
    fn open() -> io::Result<Self> {
        let mut handle = HANDLE::default();
        // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no close; `handle` is a
        // live out-pointer for the call.
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut handle) }?;
        Ok(Self(handle))
    }

    /// `TokenIsAppContainer`: a DWORD, non-zero for an `AppContainer` token.
    fn app_container(&self) -> io::Result<bool> {
        let mut answer = [0u32; 1];
        self.information(TokenIsAppContainer, &mut answer)?;
        Ok(answer[0] != 0)
    }

    /// `TokenRestrictedSids`: a `TOKEN_GROUPS`; restricted when its count is not zero.
    fn restricted(&self) -> io::Result<bool> {
        let mut needed = 0u32;
        // SAFETY: no buffer and a zero length ask only for the size, written to `needed`.
        let sized =
            unsafe { GetTokenInformation(self.0, TokenRestrictedSids, None, 0, &raw mut needed) };
        match sized {
            Err(error) if error.code() == HRESULT::from_win32(ERROR_INSUFFICIENT_BUFFER.0) => {}
            Err(error) => return Err(error.into()),
            Ok(()) => return Err(io::Error::other("TokenRestrictedSids gave no size")),
        }
        // u64 words: TOKEN_GROUPS holds pointers, so the buffer is aligned for them.
        let words = usize::try_from(needed)
            .map_err(io::Error::other)?
            .div_ceil(size_of::<u64>())
            .max(1);
        let mut buffer = vec![0u64; words];
        self.information(TokenRestrictedSids, &mut buffer)?;
        // GroupCount is TOKEN_GROUPS's first field (asserted above), and the buffer holds at least
        // one u64, so its first 4 bytes in memory order are the count.
        let first = buffer.first().copied().unwrap_or_default().to_ne_bytes();
        let count = u32::from_ne_bytes([first[0], first[1], first[2], first[3]]);
        Ok(count != 0)
    }

    /// Fills `buffer` with the token's `class` information.
    fn information<T: Copy>(
        &self,
        class: TOKEN_INFORMATION_CLASS,
        buffer: &mut [T],
    ) -> io::Result<()> {
        let len = u32::try_from(size_of_val(buffer)).map_err(io::Error::other)?;
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
        }?;
        Ok(())
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenProcessToken and is closed once, here. A failed close
        // leaves nothing to do.
        drop(unsafe { CloseHandle(self.0) });
    }
}
