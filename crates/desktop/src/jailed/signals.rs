//! The Windows check's decision, apart from its Win32 calls (`windows.rs`) so that every OS's
//! tests run it through the [`Token`] seam.
//!
//! Four signals, any one of which means the agent sandbox ([`decide`] returns the first that
//! fires): the token is an `AppContainer`, the token has restricting SIDs, the token's integrity
//! level is below Medium, or the token's user is any of S15's pool users, whose SIDs S15's set-up
//! records (`windows::sandbox_pool_sids`). Pool users always run at Low, so the integrity signal
//! still catches a stray pool process whose account was deleted and made again under a new SID,
//! or one that outlived a removal, whatever the record says. An integrity level that can't be
//! read counts as sandboxed for any failure, [`Failure::Other`] included. A read that is refused
//! (`ERROR_ACCESS_DENIED`) counts as sandboxed too ([`Signal::Unreadable`]): an unjailed
//! process can always read its own token and that record, which everyone may read, while a
//! hostile agent can deny them (by changing its default DACL first). So does a record that is not
//! a `REG_MULTI_SZ` or not a list of valid SIDs: only S15's elevated set-up writes it. A missing
//! record means no pool on this machine. Any other failure is [`Failure::Other`]: it makes the
//! check fail open (`jailed.rs`) unless another signal answers.

#[cfg(test)]
mod tests;

use std::io;

use super::Signal;

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

/// The first signal that says the process is sandboxed, from its opened `token` and the pool
/// record that `pool` reads (empty: no record), in this order: `AppContainer`, restricting SIDs,
/// below Medium, pool user. A refused read, or a record that is the wrong type or not a list of
/// SIDs, is [`Signal::Unreadable`] where it happens. The token's user is compared with every pool
/// SID until one matches or counts as sandboxed. `Err` only when no signal fired and a read
/// failed with [`Failure::Other`]; the first such error comes back.
pub(super) fn decide<T: Token>(
    token: Result<T, Failure>,
    pool: impl FnOnce() -> Result<Vec<String>, Failure>,
) -> io::Result<Option<Signal>> {
    let mut failed = None;
    match first_signal(token, pool, &mut failed) {
        Some(signal) => Ok(Some(signal)),
        None => failed.map_or(Ok(None), Err),
    }
}

/// `decide`'s walk through the signals; a [`Failure::Other`] goes to `failed` (the first
/// one kept) and the walk goes on.
fn first_signal<T: Token>(
    token: Result<T, Failure>,
    pool: impl FnOnce() -> Result<Vec<String>, Failure>,
    failed: &mut Option<io::Error>,
) -> Option<Signal> {
    let token = match token {
        Ok(token) => token,
        Err(failure) => return read(Err(failure), Signal::Unreadable, failed),
    };
    if let Some(signal) = read(token.app_container(), Signal::AppContainer, failed) {
        return Some(signal);
    }
    if let Some(signal) = read(token.restricted(), Signal::RestrictedToken, failed) {
        return Some(signal);
    }
    // Fail closed on every failure: a pool process is always Low.
    if token.below_medium().unwrap_or(true) {
        return Some(Signal::BelowMedium);
    }
    match pool() {
        Ok(sids) => sids
            .iter()
            .find_map(|sid| read(token.user_is(sid), Signal::PoolUser, failed)),
        Err(failure) => read(Err(failure), Signal::PoolUser, failed),
    }
}

/// One read's answer: `signal` when it is yes, [`Signal::Unreadable`] when it was refused or the
/// record is the wrong type or not a SID, nothing when it is no or failed otherwise (the failure
/// kept in `failed` unless one is there already).
fn read(
    answer: Result<bool, Failure>,
    signal: Signal,
    failed: &mut Option<io::Error>,
) -> Option<Signal> {
    match answer {
        Ok(found) => found.then_some(signal),
        Err(Failure::Other(error)) => {
            failed.get_or_insert(error);
            None
        }
        Err(Failure::Denied | Failure::WrongType | Failure::NotASid) => Some(Signal::Unreadable),
    }
}
