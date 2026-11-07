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
//! Default. The reset proves no earlier profile is left ([`delete`], [`check_remade`]): before
//! the logon, the listed folder and every `<profiles folder>\<name>` or `<name>.*` must be
//! absent, and the logon must make exactly `<profiles folder>\<name>`. Otherwise a folder the
//! delete left (a `ProfileList` key gone, the folder not) would stay readable to the same SID
//! while the logon made `<name>.<computer>` beside it, and a later run would take that one for
//! the profile. Nothing is deleted but through `DeleteProfileW`: an administrator deletes what
//! the failure names. Step 7's owner check and shared-write
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

/// Deletes `user`'s profile for a reset, and proves nothing of an earlier profile is left where
/// the logon could find it, without deleting anything else (an elevated recursive delete in a
/// folder the user once wrote risks its links): the folder `ProfileList` listed is gone, and
/// `<profiles folder>\<name>` and every `<profiles folder>\<name>.*` is absent, compared
/// without ASCII case. The folder the logon must then make: `<profiles folder>\<name>`. Any
/// failure is at [`Step::ResetProfile`], before the user's password changes, and names what is
/// left so an administrator can delete it.
pub(super) fn delete(machine: &mut impl Machine, user: &PoolUser) -> Result<PathBuf, Failed> {
    let failed = |cause: String| (Step::ResetProfile, format!("{}: {cause}", user.name));
    let old = machine
        .profile_of(&user.sid)
        .map_err(|error| failed(cause_of(&error)))?
        .map(|profile| profile.path);
    machine
        .delete_profile(&user.sid)
        .map_err(|error| failed(cause_of(&error)))?;
    let mut left = Vec::new();
    if let Some(old) = old {
        let there = machine
            .exists(&old)
            .map_err(|error| failed(cause_of(&error)))?;
        if there {
            left.push(old);
        }
    }
    let dir = machine
        .profiles_dir()
        .map_err(|error| failed(format!("the profiles folder: {}", cause_of(&error))))?;
    let names = machine
        .folder_names(&dir)
        .map_err(|error| failed(format!("{}: {}", dir.display(), cause_of(&error))))?;
    for name in names.iter().filter(|name| of_account(name, &user.name)) {
        let folder = dir.join(name);
        if !left.iter().any(|path| same_path(path, &folder)) {
            left.push(folder);
        }
    }
    if left.is_empty() {
        return Ok(dir.join(&user.name));
    }
    let left: Vec<String> = left.iter().map(|path| path.display().to_string()).collect();
    Err(failed(format!(
        "an earlier profile's folder is left ({}): delete it with rd /s /q from a Command Prompt \
         run as administrator, which deletes links in it as links, then run set-up again; never \
         use takeown /R or icacls /T on it, which follow links",
        left.join("; ")
    )))
}

/// Whether `folder` is a profile folder Windows could make for `account`: its name, or its name
/// and a `.` suffix (`<name>.<computer>`, `<name>.000`), without ASCII case.
fn of_account(folder: &str, account: &str) -> bool {
    let Some(head) = folder.get(..account.len()) else {
        return false;
    };
    let rest = folder.get(account.len()..).unwrap_or_default();
    head.eq_ignore_ascii_case(account) && (rest.is_empty() || rest.starts_with('.'))
}

/// Whether `a` and `b` are the same path without ASCII case, as NTFS and `ProfileList` (whose
/// `ProfileImagePath` may differ in case from `GetUserProfileDirectoryW`'s answer) take them.
fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// After a reset's logon: the profile made again must be `expected`, `<profiles folder>\<name>`
/// (without ASCII case). Otherwise [`Step::ResetProfile`], and the mark stays.
pub(super) fn check_remade(user: &PoolUser, expected: &Path, new: &Path) -> Result<(), Failed> {
    if same_path(expected, new) {
        return Ok(());
    }
    Err((
        Step::ResetProfile,
        format!(
            "{}: the logon made its profile at {}, not at {}: an earlier folder may be left",
            user.name,
            new.display(),
            expected.display()
        ),
    ))
}
