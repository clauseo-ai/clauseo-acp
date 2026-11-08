//! The WSL phase (ADR 0049, "WSL"): re-applies WSL's deny for the pool group (`wsl::apply`),
//! since a WSL update or repair rewrites its launch permissions. Three tasks run it:
//!
//! - every run of `\Clauseo\SandboxRefresh` (at the owner's logon, on demand) that is allowed
//!   (LOCAL SYSTEM, from the admin copy), twice: first, before the admin copy's steps, so the
//!   deny comes back without waiting for them; and last, whatever they gave;
//! - `\Clauseo\SandboxWslDeny` (`--wsl-deny`, [`wsl_deny`]), on Windows Installer's events
//!   in the Application log: once allowed, as the refresh is, it runs the phase, then again on
//!   the schedule in `wsl_phase/schedule.rs`: every [`WSL_RECHECK_PAUSE`] for
//!   [`WSL_RECHECKS`] more passes, and on every change under `HKLM\SOFTWARE\Classes\AppID`
//!   while a Windows Installer transaction runs and until it has been over for 3 s (an
//!   installer may write the launch permissions in more than one pass, and WSL's deletes its
//!   `AppID` keys and writes them again ~16 s later, so each pass re-reads them and re-applies
//!   what changed). It does nothing else, no admin-copy step and never WSL's upgrade. Anyone
//!   who can log on can forge such an event, so the trigger is on this task alone: a forged one
//!   gains a re-applied deny.
//!   The trigger starts it at once (no delay); a start while it runs runs beside it
//!   (`TASK_INSTANCES_PARALLEL`), finds the run lock held, asks for one more run and exits at
//!   once (`wsl_phase/run_lock.rs`): an install's event during a held run still gives a run
//!   after it, and a forged flood costs only processes that exit at once. The
//!   launch check starts it too, as the lawyer, for a missing deny (`clean_close::wsl_heal`);
//! - `\Clauseo\SandboxWslUpgrade` (`--wsl-upgrade`, [`wsl_upgrade`], `upgrade.rs`), on demand
//!   only, which the launch check alone asks for: WSL's pending MSI upgrade, verified and
//!   installed (`wsl/msi.rs`) unless a pool user has a process, then the phase. No event starts
//!   it, so no forged event picks the moment SYSTEM's msiexec runs.
//!
//! The phase is idempotent and cheap.
//!
//! Each waits up to [`WSL_LOCK_WAIT`] for the set-up lock, so a run that meets another
//! holder briefly (`--adopt`, another refresh's admin-copy steps) still re-applies; one still
//! held then is logged, and a set-up or removal holding it does its own WSL step.
//!
//! The phase's outcome never changes the exit: the launch check refuses a launch whose deny is
//! missing (`Blocked::WslDenyMissing`), so the deny's state is enforced there, and a failure here
//! is one Event Log error line, naming `AppID`s and Windows error codes, never a path or a SID.
//! A run that changed nothing writes no line.

use std::time::Duration;

use super::{Refresher, WSL_LOCK_WAIT, allowed};

/// How many timed passes `--wsl-deny` makes after its first, at least: the schedule's floor.
pub(super) const WSL_RECHECKS: u32 = 4;
/// The wait before each of them (`schedule.rs` bounds the run as a whole).
pub(super) const WSL_RECHECK_PAUSE: Duration = Duration::from_secs(5);

mod run_lock;
#[cfg(test)]
mod run_lock_tests;
mod schedule;
#[cfg(test)]
mod schedule_tests;
mod upgrade;
#[cfg(windows)]
pub(super) mod windows_lock;
#[cfg(windows)]
pub(super) mod windows_watch;

pub use run_lock::{Locked, WslRunLock};
pub use schedule::{Installer, Waited, WslWatch};
#[cfg(test)]
pub(super) use schedule::{
    WSL_ABSENT_GRACE, WSL_DENY_BUDGET, WSL_INSTALL_QUIET, WSL_WAKE_GAP, WSL_WAKE_SETTLE,
};
pub use upgrade::{WslUpgrade, WslUpgradeExit, WslUpgraded, wsl_upgrade};

/// The WSL deny task's exit codes. They start at 51, after `StopExit`'s 50, so the one exe
/// never gives one number two meanings; codes are appended from here on, never renumbered. 53 to
/// 59 are kept for #717's sweep, and the WSL upgrade task's start at 60 ([`WslUpgradeExit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WslDenyExit {
    /// 0: the run was allowed and the phase ran; its outcome is its Event Log line, as in the
    /// refresh (the launch check enforces the deny).
    Done,
    /// 51: not LOCAL SYSTEM, or not run from the admin copy's path; nothing was read or changed.
    NotAllowed,
    /// 52: "upgrade failed, deny applied", from before WSL's upgrade moved to its own task
    /// (`--wsl-upgrade`, [`WslUpgradeExit::UpgradeFailed`]). Never given now; kept so the number
    /// keeps its meaning in an older run's last result.
    UpgradeFailed,
}

impl WslDenyExit {
    /// Every exit, in order; appended with each new one.
    pub const ALL: [WslDenyExit; 3] = [
        WslDenyExit::Done,
        WslDenyExit::NotAllowed,
        WslDenyExit::UpgradeFailed,
    ];

    /// The exit whose [`as_code`](Self::as_code) is `code` (the task's last result), if any.
    pub fn from_code(code: i32) -> Option<WslDenyExit> {
        Self::ALL
            .into_iter()
            .find(|exit| i32::from(exit.as_code()) == code)
    }

    /// The process exit code.
    pub const fn as_code(self) -> u8 {
        match self {
            WslDenyExit::Done => 0,
            WslDenyExit::NotAllowed => 51,
            WslDenyExit::UpgradeFailed => 52,
        }
    }
}

/// `--wsl-deny`, the task `\Clauseo\SandboxWslDeny`'s run: once allowed (LOCAL SYSTEM, from the
/// admin copy, as the refresh's step 1), the phase, then its passes on the schedule
/// (`schedule.rs`), one run at a time (`run_lock.rs`), and nothing else: never WSL's upgrade,
/// never the pool's processes. A start that finds a run going on asks it for one more and gives
/// `Done` at once.
pub fn wsl_deny<F: WslWatch + WslRunLock>(fs: &mut F) -> WslDenyExit {
    if let Err((_, why)) = allowed(fs) {
        fs.log(true, &format!("WSL's deny wasn't re-applied: {why}"));
        return WslDenyExit::NotAllowed;
    }
    run_lock::runs(fs);
    WslDenyExit::Done
}

/// One pass's outcome.
pub(super) struct Pass {
    /// Whether every target found has the deny first now (nothing owed counts).
    pub(super) verified: bool,
    /// Its log line, if any, and whether it is an error.
    pub(super) line: Option<(bool, String)>,
}

/// Re-applies WSL's deny under the set-up lock, logging the outcome as the module says.
pub(super) fn run(fs: &mut impl Refresher) {
    if let Some((error, line)) = pass(fs).line {
        fs.log(error, &line);
    }
}

/// Re-applies WSL's deny under the set-up lock, giving what [`run`] logs.
pub(super) fn pass(fs: &mut impl Refresher) -> Pass {
    let unverified = |error, line| Pass {
        verified: false,
        line: Some((error, line)),
    };
    match fs.lock_waiting() {
        Ok(true) => {}
        Ok(false) => {
            let secs = WSL_LOCK_WAIT.as_secs();
            let line = format!("WSL's deny wasn't checked: the set-up lock was held for {secs} s");
            return unverified(false, line);
        }
        Err(why) => return unverified(true, format!("WSL's deny wasn't checked: {why}")),
    }
    let outcome = fs.reapply_wsl();
    fs.unlock();
    match outcome {
        Ok(line) => Pass {
            verified: true,
            line: line.map(|line| (false, line)),
        },
        Err(why) => unverified(true, format!("WSL's deny for the sandbox users: {why}")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake::{Fake, good, key, new, old, run_fake};
    use super::super::{RefreshExit, SourceError};
    use super::{
        WSL_DENY_BUDGET, WSL_RECHECK_PAUSE, WSL_RECHECKS, WslDenyExit, WslUpgraded, wsl_deny,
    };
    use crate::windows::refresh::test_signer::TestKey;

    /// The fake machine, ready to refresh, and the key its exe is signed with.
    fn fake() -> (Fake, TestKey) {
        let key = key();
        (Fake::new(&old(), &new(), &key, &good()), key)
    }

    /// The calls of `machine` that are the WSL phase's or the admin copy's lock.
    fn phases(machine: &Fake) -> Vec<&'static str> {
        let wanted = ["lock_waiting", "reapply_wsl", "lock", "record", "unlock"];
        let calls = machine.calls.iter().filter(|call| wanted.contains(*call));
        calls.copied().collect()
    }

    #[test]
    fn every_allowed_run_reapplies_first_and_last_under_the_lock_and_keeps_its_exit() {
        let (mut machine, key) = fake();
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::Done);
        let wsl = ["lock_waiting", "reapply_wsl", "unlock"];
        let copy = ["lock", "record", "unlock"];
        assert_eq!(phases(&machine), [&wsl[..], &copy, &wsl].concat());
        // Nothing owed: no line of its own.
        assert_eq!(machine.logs.len(), 1, "{:?}", machine.logs);
        // A failure is one error line, and the exit stays.
        let (mut machine, key) = fake();
        let why = "{370121D2-AA7E-4608-A86D-0BBAB9DA1A60}: RegSetValueExW failed (5)";
        machine.wsl = Err(why.to_owned());
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::Done);
        let (error, line) = machine.logs.last().unwrap();
        assert!(*error && line.starts_with("WSL's deny"), "{line}");
        assert!(line.ends_with(why), "{line}");
        // A change is an information line.
        let (mut machine, key) = fake();
        machine.wsl = Ok(Some(
            "WSL's deny was put back on 1 launch permission".to_owned(),
        ));
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::Done);
        assert_eq!(machine.logs.last().map(|(error, _)| *error), Some(false));
        // A refused copy still re-applies, and keeps its own exit.
        let (mut machine, key) = fake();
        machine.source_exe = Err(SourceError::Missing);
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::SourceRefused);
        let reapplied = machine.calls.iter().filter(|c| **c == "reapply_wsl");
        assert_eq!(reapplied.count(), 2);
    }

    #[test]
    fn a_run_not_allowed_leaves_wsl_alone_and_a_held_lock_is_waited_for_then_logged() {
        let (mut machine, key) = fake();
        machine.system = false;
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::NotAllowed);
        assert!(!machine.calls.contains(&"reapply_wsl"));
        // The admin copy's lock held: its steps exit 44, and WSL's phases still run.
        let (mut machine, key) = fake();
        machine.fail = Some("lock");
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::LockHeld);
        let reapplied = machine.calls.iter().filter(|c| **c == "reapply_wsl");
        assert_eq!(reapplied.count(), 2);
        // Held past the wait: each phase says so, as information, and the exit stays.
        let (mut machine, key) = fake();
        machine.fail = Some("lock_waiting");
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::Done);
        assert!(!machine.calls.contains(&"reapply_wsl"));
        let held: Vec<&(bool, String)> = machine
            .logs
            .iter()
            .filter(|(_, line)| line.contains("lock was held for 10 s"))
            .collect();
        assert_eq!(held.len(), 2, "{:?}", machine.logs);
        assert!(held.iter().all(|(error, _)| !error));
    }

    #[test]
    fn the_wsl_deny_run_reapplies_then_rechecks_and_does_nothing_else() {
        let (mut machine, _key) = fake();
        // Even with WSL's upgrade owed: the trigger task never installs it.
        machine.upgrade = WslUpgraded::Installed("installed".to_owned());
        machine.pool_busy = Ok(false);
        assert_eq!(wsl_deny(&mut machine), WslDenyExit::Done);
        let pass = ["lock_waiting", "reapply_wsl", "unlock"];
        let passes = pass.repeat(5);
        assert_eq!(phases(&machine), passes);
        // No admin-copy step: nothing read beyond the run's own check.
        let allowed = [
            "is_system",
            "current_exe",
            "admin_copy",
            "watch",
            "wait_change",
            "installer",
            "recorded_key_missing",
            "run_lock",
            "run_unlock",
            "take_again",
            "asked_again",
        ];
        let rest: Vec<&str> = machine
            .calls
            .iter()
            .copied()
            .filter(|c| !allowed.contains(c))
            .collect();
        // The event-triggered task never upgrades WSL, nor looks at the pool's processes.
        assert_eq!(rest, passes);
        assert!(machine.logs.is_empty(), "{:?}", machine.logs);
        // Not LOCAL SYSTEM: refused, nothing re-applied, one error line, its own code.
        let (mut machine, _key) = fake();
        machine.system = false;
        assert_eq!(wsl_deny(&mut machine), WslDenyExit::NotAllowed);
        assert!(!machine.calls.contains(&"reapply_wsl"));
        assert!(!machine.calls.contains(&"upgrade_wsl"));
        assert!(!machine.calls.contains(&"watch"));
        assert!(!machine.calls.contains(&"wait_change"));
        assert_eq!(machine.logs.len(), 1);
        assert!(machine.logs[0].0, "{:?}", machine.logs);
        assert_eq!(WslDenyExit::ALL.map(WslDenyExit::as_code), [0, 51, 52]);
        for exit in WslDenyExit::ALL {
            assert_eq!(WslDenyExit::from_code(exit.as_code().into()), Some(exit));
        }
    }

    #[test]
    fn the_wsl_deny_run_rechecks_after_a_pause_and_reapplies_a_second_pass() {
        let (mut machine, _key) = fake();
        // The installer writes again after the first re-apply: the second pass puts it back,
        // the later ones find it in place and write nothing.
        let put_back = "WSL's deny was put back on 1 launch permission".to_owned();
        machine.wsl_passes = vec![Ok(Some(put_back.clone())), Ok(Some(put_back)), Ok(None)];
        assert_eq!(wsl_deny(&mut machine), WslDenyExit::Done);
        assert_eq!(WSL_RECHECKS, 4);
        assert_eq!(machine.watch.waits, vec![WSL_RECHECK_PAUSE; 4]);
        assert!(machine.paused.is_empty(), "no change, no settle");
        assert_eq!(WSL_RECHECK_PAUSE.as_secs(), 5);
        let wanted = ["wait_change", "reapply_wsl"];
        let order: Vec<&str> = machine
            .calls
            .iter()
            .copied()
            .filter(|c| wanted.contains(c))
            .collect();
        let mut expected = vec!["reapply_wsl"];
        for _ in 0..4 {
            expected.extend(["wait_change", "reapply_wsl"]);
        }
        assert_eq!(order, expected);
        // One information line per pass that changed something.
        assert_eq!(machine.logs.len(), 2, "{:?}", machine.logs);
        assert!(machine.logs.iter().all(|(error, _)| !error));
        // The run ended at the last floor pass, 20 s in, well inside its budget.
        assert_eq!(machine.watch.clock.as_secs(), 20);
        assert!(machine.watch.clock < WSL_DENY_BUDGET);
    }
}
