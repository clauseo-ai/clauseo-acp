//! The Windows check's decision, apart from its Win32 calls (`windows.rs`) so that every OS's
//! tests run it through the [`Token`] seam.
//!
//! Four signals, any one of which means the agent sandbox: the token is an `AppContainer`, the
//! token has restricting SIDs, the token's integrity level is below Medium, or the token's user is
//! any of S15's pool users, whose SIDs S15's set-up records (`windows::sandbox_pool_sids`). Pool
//! users always run at Low, so the integrity signal still catches a stray pool process whose
//! account was deleted and made again under a new SID, or one that outlived a removal, whatever
//! the record says. An integrity level that can't be read counts as sandboxed for any failure,
//! [`Failure::Other`] included. A read that is refused (`ERROR_ACCESS_DENIED`)
//! counts as sandboxed too: an unjailed process can always read its own token and that record,
//! which everyone may read, while a hostile agent can deny them (by changing its default DACL
//! first). So does a record that is not a `REG_MULTI_SZ` or not a list of valid SIDs: only S15's
//! elevated set-up writes it. A missing record means no pool on this machine. Any other failure
//! is [`Failure::Other`]: it makes the check fail open (`jailed.rs`) unless another signal
//! answers.

#[cfg(test)]
mod tests;

use std::io;

/// Why a read for the check failed.
#[derive(Debug)]
pub(super) enum Failure {
    /// `ERROR_ACCESS_DENIED`: counts as sandboxed.
    Denied,
    /// The pool record is not a `REG_MULTI_SZ`: counts as sandboxed.
    WrongType,
    /// The pool record is not a list of valid SID strings: counts as sandboxed.
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
    /// `TokenIntegrityLevel`'s mandatory label has a last sub-authority below
    /// `SECURITY_MANDATORY_MEDIUM_RID` (0x2000): Low, Untrusted, or anything under Medium.
    fn below_medium(&self) -> Result<bool, Failure>;
    /// The token's user (`TokenUser`) is the SID that `sid` spells; [`Failure::NotASid`] when
    /// `sid` spells none.
    fn user_is(&self, sid: &str) -> Result<bool, Failure>;
}

/// Whether the process is sandboxed, from its opened `token` and the pool record that `pool`
/// reads (empty: no record). The token's user is compared with every pool SID until one matches
/// or counts as sandboxed. `Err` only when no signal said sandboxed and a read failed with
/// [`Failure::Other`]; the first such error comes back.
pub(super) fn decide<T: Token>(
    token: Result<T, Failure>,
    pool: impl FnOnce() -> Result<Vec<String>, Failure>,
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
                // Fail closed on every failure: a pool process is always Low.
                || token.below_medium().unwrap_or(true)
                || match pool() {
                    Ok(sids) => sids.iter().any(|sid| signal(token.user_is(sid))),
                    Err(failure) => signal(Err(failure)),
                }
        }
    };
    if found {
        return Ok(true);
    }
    failed.map_or(Ok(false), Err)
}
