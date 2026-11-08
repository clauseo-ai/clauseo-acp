//! `--wsl-deny`'s run lock (ADR 0049, "The WSL deny task"): one run at a time, and at most one
//! run owed after it, however many starts come.
//!
//! The task runs its instances in parallel (`TASK_INSTANCES_PARALLEL`): a queue had no bound but
//! the Task Scheduler's own, and any user who can write the Application log can forge the
//! installer events that start it, so a flood could have queued hours of 20 s+ runs, in a queue
//! that may be the machine's, ahead of the stop, the refresh and the heal request.
//!
//! - **The lock.** Each run opens [`LOCK_NAME`] in `%ProgramData%\Clauseo\Sandbox` with no
//!   sharing, holding the handle for the run ([`WslRunLock::run_lock`]). Never a named mutex,
//!   which a pool user could make first in `Global\`. The folders are made, checked and held as
//!   the sweep's admin-only log's are (#750's `log_path.rs` and `windows_sweeper/admin_log.rs`,
//!   ported to `windows_lock.rs`: that PR isn't in this one's base): `Clauseo` as the installer
//!   makes it ([`CLAUSEO_FOLDER_SDDL`]), `Sandbox` admin-only ([`ADMIN_FOLDER_SDDL`]), each
//!   made with its descriptor at once, then opened without following a reparse point and its
//!   owner and DACL read through the handle. The file is made with [`LOCK_SDDL`] and must be a
//!   plain file with one link that [`system_only`] keeps.
//! - **The flag.** A start that finds the lock held sets [`AGAIN_VALUE`] (`REG_DWORD` 1) in the
//!   SYSTEM-only record key and exits at once with success; it then tries the lock once more,
//!   so a run that released between the two still sees the flag or this start runs itself.
//!   The holder clears the flag as each run starts (that run covers what came before) and reads
//!   and clears it just before it would end: set, it runs once more, in what is left of
//!   [`WSL_DENY_BUDGET`]. Once the budget is spent it ends; the next trigger runs again. After
//!   the release it looks once more, for a start that found the lock held just before it.
//!   So the backlog is one flag, and a forged flood costs processes that exit at once.
//! - **A lock path that fails its checks** (a junction, a file or folder another user owns or
//!   may change): logged as an error, and the run goes on without the lock. Availability wins:
//!   the run only re-applies the deny, and unlocked runs are each bounded by the budget.

use super::super::Failed;
use super::WslWatch;
use super::schedule::{self, PASS_WORST, WSL_DENY_BUDGET};

/// The lock file's owner and DACL, given when it is made: SYSTEM, with full control alone,
/// protected.
pub(in crate::windows::refresh) const LOCK_SDDL: &str = "O:SYD:P(A;;FA;;;SY)";

/// The lock file's name in `%ProgramData%\Clauseo\Sandbox`.
pub(in crate::windows::refresh) const LOCK_NAME: &str = "wsl-deny.lock";

/// The folders below `%ProgramData%` the lock lives in, each with what it is made with when
/// missing; one there already must let only SYSTEM, Administrators or `TrustedInstaller`
/// change it (`admin_copy::level_safe`). As the sweep's admin-only log's (#750).
pub(in crate::windows::refresh) const LOCK_FOLDERS: [(&str, &str); 2] = [
    ("Clauseo", CLAUSEO_FOLDER_SDDL),
    ("Sandbox", ADMIN_FOLDER_SDDL),
];

/// `%ProgramData%\Clauseo` as the installer makes it (`crates/desktop/windows/hooks.nsh`; keep
/// the two, and #750's `log_path.rs`, the same): Users may read it.
pub(in crate::windows::refresh) const CLAUSEO_FOLDER_SDDL: &str =
    "O:BAG:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";

/// What `Clauseo\Sandbox` is made with: SYSTEM and Administrators alone.
pub(in crate::windows::refresh) const ADMIN_FOLDER_SDDL: &str =
    "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// The "run again" flag's value name in the record key, a `REG_DWORD`.
pub(in crate::windows::refresh) const AGAIN_VALUE: &str = "WslDenyAgain";

/// What [`WslRunLock::run_lock`] gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locked {
    /// The lock is this run's until [`WslRunLock::run_unlock`] (or the process ends).
    Held,
    /// Another run holds it (a sharing violation).
    Busy,
    /// The lock's folder or file failed its checks, or couldn't be opened: why.
    Refused(Failed),
}

/// The run lock and the "run again" flag, as the module says.
pub trait WslRunLock {
    /// Takes the run lock.
    fn run_lock(&mut self) -> Locked;
    /// Lets the run lock go.
    fn run_unlock(&mut self);
    /// Sets the flag.
    fn ask_again(&mut self) -> Result<(), Failed>;
    /// Clears the flag; whether it was set.
    fn take_again(&mut self) -> Result<bool, Failed>;
    /// Whether the flag is set, leaving it.
    fn asked_again(&mut self) -> Result<bool, Failed>;
}

/// `--wsl-deny`'s runs, once allowed, as the module says.
pub(super) fn runs<F: WslWatch + WslRunLock>(fs: &mut F) {
    let held = match fs.run_lock() {
        Locked::Held => true,
        Locked::Busy => match late(fs) {
            Some(held) => held,
            None => return,
        },
        Locked::Refused(why) => {
            refused(fs, &why);
            false
        }
    };
    if !held {
        schedule::passes(fs, WSL_DENY_BUDGET);
        return;
    }
    let start = fs.now();
    loop {
        // What came before this run is covered by it.
        if let Err(why) = fs.take_again() {
            fs.log(
                true,
                &format!("WSL's deny task couldn't clear its run-again flag: {why}"),
            );
        }
        let left = |fs: &mut F| WSL_DENY_BUDGET.saturating_sub(fs.now().saturating_sub(start));
        let budget = left(fs);
        let in_time = schedule::passes(fs, budget) && left(fs) > PASS_WORST;
        let again = match fs.take_again() {
            Ok(again) => again,
            Err(why) => {
                fs.log(
                    true,
                    &format!("WSL's deny task couldn't read its run-again flag: {why}"),
                );
                false
            }
        };
        if again && in_time {
            continue;
        }
        if again {
            // The budget is spent: this process ends inside the task's limit, and the next
            // trigger (an install's event, or the launch check's heal request) runs again.
            fs.log(
                false,
                "WSL's deny task was asked to run again after its time limit; the next start will",
            );
        }
        fs.run_unlock();
        if !in_time {
            return;
        }
        // A start that found the lock held just before the release: its flag is left for
        // whichever run holds the lock now, or this one takes it again.
        if !matches!(fs.asked_again(), Ok(true)) || fs.run_lock() != Locked::Held {
            return;
        }
    }
}

/// A start that found the lock held: sets the flag and tries the lock once more; whether it
/// runs, and with the lock (`None`: it exits at once).
fn late<F: WslWatch + WslRunLock>(fs: &mut F) -> Option<bool> {
    if let Err(why) = fs.ask_again() {
        // The running one goes on; only a request made now could go unseen.
        let line =
            format!("WSL's deny task found a run going on and couldn't ask it for another: {why}");
        fs.log(true, &line);
        return None;
    }
    match fs.run_lock() {
        Locked::Held => Some(true),
        Locked::Busy => None,
        Locked::Refused(why) => {
            refused(fs, &why);
            Some(false)
        }
    }
}

/// The lock refused: logged, and the run goes on without it.
fn refused(fs: &mut impl WslWatch, why: &str) {
    let line = format!("WSL's deny task runs without its run lock, which failed its checks: {why}");
    fs.log(true, &line);
}

/// Whether the self-relative descriptor `bytes` keeps the lock file to SYSTEM: SYSTEM its
/// owner, and a protected DACL of exactly one allow, full control (`FA`) for SYSTEM.
#[cfg(any(windows, test))]
pub(in crate::windows::refresh) fn system_only(bytes: &[u8]) -> bool {
    use crate::windows::acl_bytes::{
        ALLOWED, Acl, Descriptor, kind, mask, owner_sid, sid, sid_text,
    };

    const SYSTEM: &str = "S-1-5-18";
    if owner_sid(bytes).is_none_or(|owner| owner.as_str() != SYSTEM) {
        return false;
    }
    let Ok(descriptor) = Descriptor::parse(bytes) else {
        return false;
    };
    let Some(dacl) = descriptor.dacl.filter(|_| descriptor.dacl_protected()) else {
        return false;
    };
    let Ok(dacl) = Acl::parse(dacl) else {
        return false;
    };
    let [ace] = dacl.aces() else {
        return false;
    };
    let named = sid(ace).and_then(sid_text);
    named.is_some_and(|named| named.as_str() == SYSTEM)
        && kind(ace) == ALLOWED
        && mask(ace) == Some(0x001F_01FF)
}
