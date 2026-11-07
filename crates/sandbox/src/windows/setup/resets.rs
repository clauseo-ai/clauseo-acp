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
//! Default. The reset proves the old profile is gone ([`delete`], [`check_remade`]): its folder
//! must be gone after the delete, and the logon must give the same folder back. Otherwise a folder
//! the delete left (a `ProfileList` key gone, the folder not) would stay readable to the same SID
//! while the logon made `<name>.<computer>` beside it. Step 7's owner check and shared-write
//! denies then close its new `LocalLow` with every other user's. Only once `secrets.bin` holds
//! the new password are the marks cleared ([`clear`]). A reset that fails fails the set-up at its
//! step and leaves the mark, so the user stays held back; a clear that fails is reported and
//! leaves the marks too.
//!
//! A never-used user has no mark, nor has a user its matter still holds: neither is reset.

use std::path::{Path, PathBuf};

use super::create::Failed;
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

/// Deletes `user`'s profile for a reset: reads where `ProfileList` says it is, deletes it, and
/// checks its folder is gone. The folder, or `None` when `ProfileList` had none. Any failure is
/// at [`Step::ResetProfile`], before the user's password changes.
pub(super) fn delete(
    machine: &mut impl Machine,
    user: &PoolUser,
) -> Result<Option<PathBuf>, Failed> {
    let failed = |cause: String| (Step::ResetProfile, format!("{}: {cause}", user.name));
    let old = machine
        .profile_of(&user.sid)
        .map_err(|error| failed(cause_of(&error)))?
        .map(|profile| profile.path);
    machine
        .delete_profile(&user.sid)
        .map_err(|error| failed(cause_of(&error)))?;
    if let Some(old) = &old {
        let left = machine
            .exists(old)
            .map_err(|error| failed(cause_of(&error)))?;
        if left {
            let what = format!("its profile folder {} is still there", old.display());
            return Err(failed(what));
        }
    }
    Ok(old)
}

/// After a reset's logon: the profile made again must be `old`'s folder, or, when
/// `ProfileList` had none, the folder named for the account, which Windows uses only when no
/// folder by that name is left. Otherwise [`Step::ResetProfile`], and the mark stays.
pub(super) fn check_remade(user: &PoolUser, old: Option<&Path>, new: &Path) -> Result<(), Failed> {
    let same = match old {
        Some(old) => new == old,
        None => new
            .file_name()
            .is_some_and(|leaf| leaf.eq_ignore_ascii_case(&user.name)),
    };
    if same {
        return Ok(());
    }
    let was = old.map_or_else(|| user.name.clone(), |old| old.display().to_string());
    Err((
        Step::ResetProfile,
        format!(
            "{}: the logon made its profile at {}, not at {was}: the old folder may be left",
            user.name,
            new.display()
        ),
    ))
}
