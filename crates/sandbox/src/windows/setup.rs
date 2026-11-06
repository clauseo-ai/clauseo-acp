//! The elevated set-up's engine (S15 `PR4d`, design B): the order of its steps and the rollback
//! of a failed run, with no Windows call. Every call outside the permission writes goes through
//! [`Machine`] (`machine.rs`); the writes go through [`AclWriter`]. `WindowsMachine` (`PR4e` to
//! `PR4g`) is the real machine, and the setup exe (`PR4h`, `exe.rs`) runs [`set_up`] elevated.
//!
//! [`set_up`] takes the machine-wide lock, then:
//!
//! 1. reads `HKLM\SOFTWARE\Clauseo\Sandbox` and refuses when another user owns it (D5), with that
//!    user's SID in the cause;
//! 2. preflight: the Secondary Logon service is not disabled, and local policy doesn't refuse
//!    `Users` the interactive logon right; either fails with a named cause before any account
//!    exists;
//! 3. the group `ClauseoSandboxUsers`: made, or adopted only when its SID is the record's;
//! 4. per new user: a free name (an account outside the record with a pool name is skipped,
//!    never adopted, D-B5), a random password, the account, its groups, hidden from the sign-in
//!    screen;
//! 5. the record with the new SID; a failed write rolls the user back;
//! 6. one logon with its profile loaded; a temporary profile is a failure;
//! 7. the shared-write denies over `DeviceSync` and every pool user's `LocalLow`; any failed
//!    target is a failure, and what was written stays (D12);
//! 8. when this run made users and matters are registered, every registered matter fenced
//!    against the grown pool (`fence_registered`);
//! 9. the change journal of each chosen volume enlarged (D-B12); a failure here is listed and
//!    never fails the set-up, since it only costs full walks;
//! 10. `secrets.bin`, protected as the owner;
//! 11. `setup.json`, last, as the owner.
//!
//! A re-run on a pool this owner set up (D-B6) keeps the recorded accounts that exist, resets
//! their passwords, logs each on again and re-applies the shared writes; it makes again each
//! user whose account is gone, under a new SID, and drops a dead SID from the record only once no
//! ledger entry names it. It refuses while any session is open. An extend (`add` above 0) adds
//! users and never resets an existing password.
//!
//! A failure rolls back what this run made (`rollback.rs`, D-B7) and reports the step
//! ([`SetupFailure`]). No record write ever puts the owner's SID in `PoolSids`.
//!
//! Pure apart from its seams: built for Windows and for the tests on every OS.

use std::path::PathBuf;

use crate::windows::fence::RegisteredMatter;
use crate::windows::sids::Sid;
use crate::windows::writer::AclWriter;

#[cfg(test)]
#[cfg(windows)]
mod account_list;
#[cfg(windows)]
mod accounts;
#[cfg(test)]
#[cfg(windows)]
mod accounts_tests;
pub mod answers;
pub mod args;
#[cfg(windows)]
pub mod around;
mod create;
#[cfg(windows)]
pub mod exe;
#[cfg(windows)]
mod ffi;
#[cfg(windows)]
pub mod journal;
pub mod journal_plan;
#[cfg(windows)]
mod lock;
#[cfg(test)]
#[cfg(windows)]
mod lock_tests;
#[cfg(windows)]
pub mod logon;
mod machine;
mod outcome;
#[cfg(any(windows, test))]
pub mod owner;
pub mod password;
#[cfg(windows)]
pub mod preflight;
#[cfg(windows)]
pub mod processes;
#[cfg(windows)]
pub mod profile;
pub mod record_values;
#[cfg(windows)]
mod record_write;
pub mod registered;
pub mod result_file;
mod rollback;
#[cfg(any(windows, test))]
mod shared;
pub mod strays;
#[cfg(windows)]
mod windows_machine;

pub use machine::{LogonRight, Machine, MachineError, Password, ProfileDir, ServiceStart};
pub use outcome::{
    ERROR_ACCESS_DISABLED_BY_POLICY, ERROR_CANCELLED, ERROR_LOGON_TYPE_NOT_GRANTED, JournalOutcome,
    LOGON_NOT_GRANTED, LOGON_REFUSED, SECLOGON_DISABLED, SetupExit, SetupFailure, SetupReport,
    Step, launch_error,
};
#[cfg(windows)]
pub use windows_machine::{Names, WindowsMachine};

/// How many pool users a first set-up makes, and how many an extend adds by default (D-B5).
pub const POOL_START: usize = 4;
/// The change journal size set-up gives a volume whose journal is smaller: 256 MiB (0049).
pub const JOURNAL_MAX: u64 = 256 * 1024 * 1024;
/// The change journal's allocation delta set-up gives with it: 32 MiB.
pub const JOURNAL_DELTA: u64 = 32 * 1024 * 1024;

/// What a set-up is asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupRequest {
    /// The Windows user who asked: the parent process's token user (D-B2), never an argument.
    pub owner: Sid,
    /// The owner's data folder, which holds `sandbox\`.
    pub data_dir: PathBuf,
    /// 0 for a set-up or re-run; above 0, an extend by that many users.
    pub add: u8,
    /// Every registered matter with its pool user, for step 8's fence.
    pub matters: Vec<RegisteredMatter>,
}

/// Runs the set-up as the module says, under the machine-wide lock, which a guard gives back
/// on every return. A held lock is [`SetupExit::LockHeld`]; another owner
/// [`SetupExit::OtherOwner`]; any other failure [`SetupExit::Failed`], rolled back. A request
/// whose owner isn't the machine's (the verified parent's token user, D-B2) fails at
/// [`Step::Owner`] before the lock, so no argument decides the owner alone.
pub fn set_up(
    machine: &mut impl Machine,
    acl: &mut impl AclWriter,
    req: &SetupRequest,
) -> Result<SetupReport, SetupFailure> {
    if let Some(failure) = owner_failure(req, machine.owner()) {
        return Err(failure);
    }
    if let Err(error) = machine.lock() {
        let exit = if error == MachineError::LockHeld {
            SetupExit::LockHeld
        } else {
            SetupExit::Failed
        };
        return Err(SetupFailure {
            step: Step::Lock,
            cause: outcome::cause_of(&error),
            rolled_back: true,
            exit,
        });
    }
    let locked = Locked(machine);
    create::run(&mut *locked.0, acl, req)
}

/// The failure for a request whose owner isn't `owner`, the user who asked (`Owner::sid`,
/// D-B2): [`Step::Owner`], [`SetupExit::Failed`], nothing touched. `None` when they match. The
/// setup exe checks it when it builds the request, and [`set_up`] again against its machine.
pub fn owner_failure(req: &SetupRequest, owner: &Sid) -> Option<SetupFailure> {
    (req.owner != *owner).then(|| SetupFailure {
        step: Step::Owner,
        cause: format!(
            "the set-up was asked for {}, but the user who asked is {owner}",
            req.owner
        ),
        rolled_back: true,
        exit: SetupExit::Failed,
    })
}

/// The machine while set-up holds its lock: dropping it gives the lock back, so no return path
/// leaves the lock held.
struct Locked<'m, M: Machine>(&'m mut M);

impl<M: Machine> Drop for Locked<'_, M> {
    fn drop(&mut self) {
        self.0.unlock();
    }
}

#[cfg(test)]
mod tests;
