//! The Windows check's decision, apart from its Win32 calls (`windows.rs`) so that every OS's
//! tests run it through the [`Token`] seam.
//!
//! Three signals, any one of which means the agent sandbox: the token is an `AppContainer`, the
//! token has restricting SIDs, or the token's user is S15's sandbox user, whose SID S15's set-up
//! records (`windows::sandbox_user_sid`). A read that is refused (`ERROR_ACCESS_DENIED`) counts
//! as sandboxed too: an unjailed process can always read its own token and that record, which
//! everyone may read, while a hostile agent can deny them (by changing its default DACL first).
//! So does a record that is not a `REG_SZ` or not a valid SID: only S15's elevated set-up writes
//! it. A missing record means no sandbox user on this machine. Any other failure is
//! [`Failure::Other`]: it makes the check fail open (`jailed.rs`) unless another signal answers.

#[cfg(test)]
mod tests;

use std::io;

/// Why a read for the check failed.
#[derive(Debug)]
pub(super) enum Failure {
    /// `ERROR_ACCESS_DENIED`: counts as sandboxed.
    Denied,
    /// The sandbox user record is not a `REG_SZ`: counts as sandboxed.
    WrongType,
    /// The sandbox user record is not a valid SID string: counts as sandboxed.
    NotASid,
    /// Anything else: the check cannot tell.
    Other(io::Error),
}

/// What the check reads from this process's token. A seam, so tests can give each answer.
pub(super) trait Token {
    /// `TokenIsAppContainer`.
    fn app_container(&self) -> Result<bool, Failure>;
    /// `TokenRestrictedSids` is not empty.
    fn restricted(&self) -> Result<bool, Failure>;
    /// The token's user (`TokenUser`) is the SID that `sid` spells; [`Failure::NotASid`] when
    /// `sid` spells none.
    fn user_is(&self, sid: &str) -> Result<bool, Failure>;
}

/// Whether the process is sandboxed, from its opened `token` and the sandbox user record that
/// `sandbox_user` reads (`Ok(None)`: no record). `Err` only when no signal said sandboxed and a
/// read failed with [`Failure::Other`]; the first such error comes back.
pub(super) fn decide<T: Token>(
    token: Result<T, Failure>,
    sandbox_user: impl FnOnce() -> Result<Option<String>, Failure>,
) -> io::Result<bool> {
    let mut failed = None;
    let mut signal = |answer: Result<bool, Failure>| match answer {
        Ok(found) => found,
        Err(Failure::Other(error)) => {
            failed.get_or_insert(error);
            false
        }
        Err(Failure::Denied | Failure::WrongType | Failure::NotASid) => true,
    };
    let found = match token {
        Err(failure) => signal(Err(failure)),
        Ok(token) => {
            signal(token.app_container())
                || signal(token.restricted())
                || match sandbox_user() {
                    Ok(None) => false,
                    Ok(Some(sid)) => signal(token.user_is(&sid)),
                    Err(failure) => signal(Err(failure)),
                }
        }
    };
    if found {
        return Ok(true);
    }
    failed.map_or(Ok(false), Err)
}
