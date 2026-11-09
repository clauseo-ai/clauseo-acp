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
//! 3. the group `ClauseoSandboxUsers`: made, or adopted only when its SID is the record's; then,
//!    every run, granted "Deny log on through Remote Desktop Services", so no pool user can sign
//!    in over Remote Desktop (a rollback that deletes the group takes the right off first);
//! 4. per new user: a free name (an account outside the record with a pool name is skipped,
//!    never adopted, D-B5), a random password, the account, its groups, hidden from the sign-in
//!    screen;
//! 5. the record with the new SID; a failed write rolls the user back;
//! 6. one logon with its profile loaded; a temporary profile is a failure;
//! 7. the shared-write denies over `DeviceSync` and every pool user's `LocalLow`; any failed
//!    target is a failure, and what was written stays (D12);
//! 8. when this run made users and matters are registered, every registered matter fenced
//!    against the grown pool (`fence_registered`); the matters are read from the owner's
//!    `matters.json` and ledger right after step 1, under the lock, as the owner;
//! 9. the change journal of each chosen volume enlarged (D-B12); a failure here is listed and
//!    never fails the set-up, since it only costs full walks;
//! 10. `secrets.bin`, protected as the owner;
//! 11. `setup.json`, last, as the owner.
//!
//! Between steps 8 and 9, step 8a (S15 PR7, `wfp`) writes the network filters: WFP block filters
//! naming every live pool user, the owner's grants that let a launch add its own permit, and the
//! firewall rules. A WFP failure fails the set-up; a firewall Group Policy ignores is only
//! reported.
//!
//! A re-run on a pool this owner set up (D-B6) keeps the recorded accounts that exist, resets
//! their passwords, logs each on again and re-applies the shared writes; it makes again each
//! user whose account is gone, under a new SID, and drops a dead SID from the record only once no
//! ledger entry names it. It refuses while any session is open. An extend (`add` above 0) adds
//! users and never resets an existing password, but for a user owed a profile reset.
//!
//! Every run then tops the pool up to [`MIN_FREE_AFTER_RUN`] free users, cap allowing
//! (`top_up.rs`): a fresh pool is 8, an extend's `add` is a minimum, and a re-run stays a
//! re-run (every kept password reset) and makes the shortfall too.
//!
//! Both reset each recorded user released from a matter (`resets.rs`): its profile deleted, a new
//! password, one logon that makes the profile again, its `LocalLow` closed by step 7's owner
//! check and denies; its mark in the owner's `matters.json` is cleared after step 10.
//!
//! A failure rolls back what this run made (`rollback.rs`, D-B7) and reports the step
//! ([`SetupFailure`]). No record write ever puts the owner's SID in `PoolSids`.
//!
//! The uninstall's `--remove` (`PR4j`) is [`remove()`], over its own seam
//! ([`remove::Removal`], `remove.rs`).
//!
//! Pure apart from its seams: built for Windows and for the tests on every OS.

use std::path::PathBuf;

use crate::windows::sids::Sid;
use crate::windows::writer::AclWriter;

#[cfg(test)]
#[cfg(windows)]
pub(crate) mod account_list;
#[cfg(windows)]
pub(crate) mod accounts;
#[cfg(test)]
#[cfg(windows)]
mod accounts_tests;
#[cfg(windows)]
mod admin_member;
#[cfg(test)]
#[cfg(windows)]
mod admin_record_tests;
#[cfg(windows)]
mod admin_remove;
#[cfg(any(windows, test))]
pub mod adopt;
pub mod answers;
pub mod args;
#[cfg(windows)]
pub mod around;
mod create;
#[cfg(windows)]
mod event_trace;
#[cfg(windows)]
pub mod exe;
#[cfg(any(windows, test))]
pub mod explain;
#[cfg(windows)]
mod ffi;
#[cfg(test)]
#[cfg(windows)]
mod handback_tests;
#[cfg(windows)]
pub mod journal;
pub mod journal_plan;
mod late_steps;
#[cfg(windows)]
pub(crate) mod lock;
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
#[cfg(any(windows, test))]
pub mod pool_steps;
#[cfg(windows)]
pub mod preflight;
#[cfg(windows)]
pub mod processes;
#[cfg(windows)]
pub mod profile;
#[cfg(windows)]
mod reclaim_run;
#[cfg(test)]
#[cfg(windows)]
mod reclaim_tests;
#[cfg(any(windows, test))]
mod record_key;
pub mod record_values;
#[cfg(windows)]
mod record_write;
#[cfg(windows)]
pub(crate) use record_write::{read_record_install_id, read_record_value};
pub mod registered;
pub mod remove;
mod resets;
pub mod result_file;
#[cfg(test)]
#[cfg(windows)]
mod rights_tests;
mod rollback;
#[cfg(any(windows, test))]
mod shared;
#[cfg(any(windows, test))]
pub mod stop;
pub mod strays;
#[cfg(any(windows, test))]
pub mod sweep;
#[cfg(any(windows, test))]
pub mod taken_file;
mod top_up;
// The SYSTEM tasks' comparison is `refresh`'s, which builds on Windows and for the tests.
#[cfg(any(windows, test))]
mod user_tasks;
#[cfg(windows)]
mod windows_machine;

pub use machine::{LogonRight, Machine, MachineError, Password, ProfileDir, ServiceStart};
pub use outcome::{
    APPROVER_LOGON_REFUSED, DATA_DIR_NOT_OWNED, ERROR_ACCESS_DISABLED_BY_POLICY, ERROR_CANCELLED,
    ERROR_LOGON_TYPE_NOT_GRANTED, ERROR_VIRUS_DELETED, ERROR_VIRUS_INFECTED, JournalOutcome,
    LOGON_NOT_GRANTED, LOGON_REFUSED, SECLOGON_DISABLED, SetupExit, SetupFailure, SetupReport,
    Step, TASKS_NOT_PUT_BACK, TopUp, exit_of, launch_code, launch_error,
};
// `remove::Removal` stays at its path: the set-up's fake implements both seams, and a glob
// import of both would make their shared method names ambiguous in the set-up's tests.
pub use remove::{
    LedgerWork, REMOVE_LOCK_WAIT_MS, RemoveFailure, RemoveReport, RemoveStep, Removed, remove,
    remove_then,
};
#[cfg(windows)]
pub use windows_machine::{Names, WindowsMachine};

/// How many pool users an extend adds by default, and the fewest a set-up makes before the
/// top-up (D-B5); [`MIN_FREE_AFTER_RUN`] makes a fresh pool 8.
pub const POOL_START: usize = 4;
/// The fewest free pool users every elevated run leaves, cap allowing (`top_up.rs`).
pub const MIN_FREE_AFTER_RUN: usize = 8;
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
    /// 0 for a set-up or re-run; above 0, an extend by that many users. The registered matters
    /// step 8 fences are read from the owner's files once the lock is held
    /// (`Machine::registered_matters`), never taken from the caller.
    pub add: u8,
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
