//! The profile resets a set-up owes: the pool users released from a matter, whose profile,
//! `LocalLow` and hive may still hold what the old matter's agent wrote there. A Low token can
//! write `HKCU\Software\AppDataLow` and any other Low-labelled place in its own profile, which no
//! hand-back reaches, so `MatterSids::release` marks the user in the owner's `matters.json` and
//! the reuse rule holds it back (`Why::NeedsReset`) until a set-up resets it.
//!
//! A set-up over a recorded pool (an extend or a re-run) reads the marks right after the
//! registered matters, as the owner, under the lock ([`resets_due`]). Each marked user whose
//! account exists is reset in `keep_user`: its profile deleted (`Machine::delete_profile`, which
//! takes the hive with it), a new password, then one logon that makes the profile again from
//! Default. step 7's owner check and shared-write denies then close its new `LocalLow` with every
//! other user's. Only once `secrets.bin` holds the new password are the marks cleared
//! ([`clear`]). A reset that fails fails the set-up at its step and leaves the mark, so the user
//! stays held back; a clear that fails is reported and leaves the marks too.
//!
//! A never-used user has no mark, nor has a user its matter still holds: neither is reset.

use super::machine::{Machine, owned};
use super::outcome::{SetupFailure, Step, cause_of};
use crate::windows::pool::{MachineRecord, PoolUser};
use crate::windows::sids::Sid;

/// The pool users the owner's `matters.json` marks as owed a reset and no matter holds, read as
/// the owner (`Machine::resets_due`); none without a record, as a fresh pool has no users to
/// reset. A file that can't be read fails at [`Step::Matters`], before anything changes.
pub(super) fn resets_due(
    machine: &mut impl Machine,
    record: Option<&MachineRecord>,
) -> Result<Vec<Sid>, SetupFailure> {
    if record.is_none() {
        return Ok(Vec::new());
    }
    owned(machine, Machine::resets_due).map_err(|error| {
        SetupFailure::early(
            Step::Matters,
            format!("the profile resets owed: {}", cause_of(&error)),
        )
    })
}

/// Clears the marks of `reset`, the users this run reset, as the owner (`Machine::reset_done`):
/// those users, or why the marks stay (the users stay held back until a set-up clears them).
pub(super) fn clear(
    machine: &mut impl Machine,
    reset: Vec<PoolUser>,
) -> Result<Vec<PoolUser>, String> {
    if reset.is_empty() {
        return Ok(reset);
    }
    let sids: Vec<Sid> = reset.iter().map(|user| user.sid.clone()).collect();
    match owned(machine, |machine| machine.reset_done(&sids)) {
        Ok(()) => Ok(reset),
        Err(error) => {
            tracing::warn!(
                users = reset.len(),
                "sandbox users' profiles were reset, but their marks couldn't be cleared"
            );
            Err(cause_of(&error))
        }
    }
}
