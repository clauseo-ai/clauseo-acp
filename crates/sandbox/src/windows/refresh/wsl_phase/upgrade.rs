//! `--wsl-upgrade`, the task `\Clauseo\SandboxWslUpgrade`'s run (ADR 0049, "WSL",
//! "Self-healing"): WSL's pending MSI upgrade, then the phase. Only the launch check asks for it
//! (`clean_close::wsl_heal`), on demand: no trigger starts it, so no forged Windows Installer
//! event picks the moment SYSTEM's msiexec runs.
//!
//! Once allowed (LOCAL SYSTEM, from the admin copy), it first looks for a pool user's live
//! process, through the stop's process snapshot ([`WslUpgrade::pool_busy`]): with one, or with
//! the processes unreadable, the MSI isn't started (its rewrite would drop the deny while that
//! user runs), the phase still runs, and the run exits [`WslUpgradeExit::Deferred`]. Otherwise
//! the upgrade, when one is owed ([`WslUpgrade::upgrade_wsl`], `wsl/msi.rs`, which verifies the
//! MSI first), then the phase, which puts back the deny the upgrade's rewrite dropped. A failed
//! upgrade leaves WSL as it was (Windows Installer rolls it back), the phase still runs, and the
//! run exits [`WslUpgradeExit::UpgradeFailed`]. The launch check takes "deferred" and "failed"
//! as "the deny is applied on the current WSL" once it has verified the deny itself.

use super::super::{Failed, Refresher, allowed};
use super::run;

/// The WSL upgrade task's exit codes: 0, 60 to 62 and 67 (the WSL deny task's are 51 and 52, 53
/// to 59 are #717's sweep's, and 63 the set-up's), so the one exe never gives one number two
/// meanings; appended from here on, never renumbered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WslUpgradeExit {
    /// 0: allowed; the upgrade was installed, or none was owed; then the phase ran.
    Done,
    /// 60: not LOCAL SYSTEM, or not run from the admin copy's path; nothing was read or changed.
    NotAllowed,
    /// 61: "deferred": a pool user had a live process (or the processes couldn't be listed), so
    /// the MSI wasn't started; the phase ran.
    Deferred,
    /// 62: "upgrade failed, deny applied": the upgrade was owed and failed (or couldn't be tried,
    /// its MSI refused included), WSL is as it was, and the phase ran after it.
    UpgradeFailed,
    /// 67 (63 is the set-up's `SetupExit`; 67 is this task's): "still installing": msiexec
    /// hadn't ended at the end of its wait (`wsl::msi::MSI_WAIT`) and was left running, so it may
    /// still rewrite the launch permissions; the phase ran, but the launch check refuses until a
    /// later run.
    StillInstalling,
}

impl WslUpgradeExit {
    /// Every exit, in order; appended with each new one.
    pub const ALL: [WslUpgradeExit; 5] = [
        WslUpgradeExit::Done,
        WslUpgradeExit::NotAllowed,
        WslUpgradeExit::Deferred,
        WslUpgradeExit::UpgradeFailed,
        WslUpgradeExit::StillInstalling,
    ];

    /// The exit whose [`as_code`](Self::as_code) is `code` (the task's last result), if any.
    pub fn from_code(code: i32) -> Option<WslUpgradeExit> {
        Self::ALL
            .into_iter()
            .find(|exit| i32::from(exit.as_code()) == code)
    }

    /// The process exit code.
    pub const fn as_code(self) -> u8 {
        match self {
            WslUpgradeExit::Done => 0,
            WslUpgradeExit::NotAllowed => 60,
            WslUpgradeExit::Deferred => 61,
            WslUpgradeExit::UpgradeFailed => 62,
            WslUpgradeExit::StillInstalling => 67,
        }
    }
}

/// What the run's upgrade step did ([`WslUpgrade::upgrade_wsl`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WslUpgraded {
    /// No upgrade pending (or none this run can find): nothing was run.
    NotPending,
    /// WSL's pending MSI upgrade was installed: the Event Log's line.
    Installed(String),
    /// It was owed and failed, or couldn't be tried: why, with codes, never a path.
    Failed(String),
    /// msiexec was left running at the end of its wait: why, never a path.
    StillInstalling(String),
}

/// The run's machine besides the phase: the machine on Windows, a fake in the tests.
pub trait WslUpgrade {
    /// Whether any of the record's pool users (`PoolSids`) has a live process, as the stop's
    /// process snapshot reads each process's user; an error when that can't be told.
    fn pool_busy(&mut self) -> Result<bool, Failed>;
    /// Installs the upgrade when one is pending, as WSL's own installer installs it
    /// (`wsl/msi.rs`), as LOCAL SYSTEM; nothing while a removal is under way.
    fn upgrade_wsl(&mut self) -> WslUpgraded;
}

/// `--wsl-upgrade`, as the module says. An installed upgrade is an information line, a deferred
/// one an information line (an error line when the processes couldn't be listed), a failed one an
/// error line.
pub fn wsl_upgrade<F: Refresher + WslUpgrade>(fs: &mut F) -> WslUpgradeExit {
    if let Err((_, why)) = allowed(fs) {
        fs.log(
            true,
            &format!("WSL's pending update wasn't looked at: {why}"),
        );
        return WslUpgradeExit::NotAllowed;
    }
    let deferred = match fs.pool_busy() {
        Ok(false) => None,
        Ok(true) => Some((false, "a sandbox user has a process running".to_owned())),
        Err(why) => Some((true, format!("the sandbox users' processes: {why}"))),
    };
    if let Some((error, why)) = deferred {
        fs.log(
            error,
            &format!("WSL's pending update wasn't started: {why}"),
        );
        run(fs);
        return WslUpgradeExit::Deferred;
    }
    let upgraded = fs.upgrade_wsl();
    match &upgraded {
        WslUpgraded::NotPending => {}
        WslUpgraded::Installed(line) => fs.log(false, line),
        WslUpgraded::Failed(why) => {
            let line = format!("WSL's pending update wasn't installed; WSL is as it was: {why}");
            fs.log(true, &line);
        }
        WslUpgraded::StillInstalling(why) => {
            fs.log(
                true,
                &format!("WSL's pending update is still installing: {why}"),
            );
        }
    }
    run(fs);
    match upgraded {
        WslUpgraded::Failed(_) => WslUpgradeExit::UpgradeFailed,
        WslUpgraded::StillInstalling(_) => WslUpgradeExit::StillInstalling,
        WslUpgraded::NotPending | WslUpgraded::Installed(_) => WslUpgradeExit::Done,
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::fake::{Fake, good, key, new, old};
    use super::{WslUpgradeExit, WslUpgraded, wsl_upgrade};

    /// The fake machine, allowed, with no pool process.
    fn fake() -> Fake {
        Fake::new(&old(), &new(), &key(), &good())
    }

    /// The calls of `machine` that are the upgrade's or the phase's.
    fn steps(machine: &Fake) -> Vec<&'static str> {
        let wanted = [
            "pool_busy",
            "upgrade_wsl",
            "lock_waiting",
            "reapply_wsl",
            "unlock",
        ];
        let calls = machine.calls.iter().filter(|call| wanted.contains(*call));
        calls.copied().collect()
    }

    #[test]
    fn a_pending_upgrade_is_installed_then_the_deny_is_reapplied() {
        let mut machine = fake();
        let line = "WSL's pending update was installed: 2.6.1 to 2.6.3";
        machine.upgrade = WslUpgraded::Installed(line.to_owned());
        assert_eq!(wsl_upgrade(&mut machine), WslUpgradeExit::Done);
        let order = [
            "pool_busy",
            "upgrade_wsl",
            "lock_waiting",
            "reapply_wsl",
            "unlock",
        ];
        assert_eq!(steps(&machine), order);
        assert_eq!(machine.logs, [(false, line.to_owned())]);
    }

    #[test]
    fn a_pool_process_defers_the_upgrade_and_the_deny_is_still_reapplied() {
        let mut machine = fake();
        machine.upgrade = WslUpgraded::Installed("never".to_owned());
        machine.pool_busy = Ok(true);
        assert_eq!(wsl_upgrade(&mut machine), WslUpgradeExit::Deferred);
        let order = ["pool_busy", "lock_waiting", "reapply_wsl", "unlock"];
        assert_eq!(steps(&machine), order);
        let (error, line) = &machine.logs[0];
        assert!(
            !error && line.ends_with("a sandbox user has a process running"),
            "{line}"
        );
        // Processes that can't be listed defer it too, as an error line.
        let mut machine = fake();
        machine.pool_busy = Err("CreateToolhelp32Snapshot failed (5)".to_owned());
        assert_eq!(wsl_upgrade(&mut machine), WslUpgradeExit::Deferred);
        assert!(!machine.calls.contains(&"upgrade_wsl"));
        assert!(machine.logs[0].0, "{:?}", machine.logs);
    }

    #[test]
    fn a_failed_upgrade_still_reapplies_the_deny_and_exits_62() {
        let mut machine = fake();
        machine.upgrade = WslUpgraded::Failed("msiexec ended with 1603".to_owned());
        assert_eq!(wsl_upgrade(&mut machine), WslUpgradeExit::UpgradeFailed);
        let order = [
            "pool_busy",
            "upgrade_wsl",
            "lock_waiting",
            "reapply_wsl",
            "unlock",
        ];
        assert_eq!(steps(&machine), order);
        let (error, line) = &machine.logs[0];
        let tail = "WSL is as it was: msiexec ended with 1603";
        assert!(*error && line.ends_with(tail), "{line}");
        // The phase failing too keeps 62: the launch check verifies the deny itself.
        let mut machine = fake();
        machine.upgrade = WslUpgraded::Failed("msiexec ended with 1618".to_owned());
        machine.wsl = Err("RegSetValueExW failed (5)".to_owned());
        assert_eq!(wsl_upgrade(&mut machine), WslUpgradeExit::UpgradeFailed);
        assert_eq!(machine.logs.len(), 2, "{:?}", machine.logs);
    }

    #[test]
    fn msiexec_left_running_exits_67_after_the_phase() {
        let mut machine = fake();
        let why = "msiexec didn't end within 240 s; it was left running";
        machine.upgrade = WslUpgraded::StillInstalling(why.to_owned());
        assert_eq!(wsl_upgrade(&mut machine), WslUpgradeExit::StillInstalling);
        assert!(machine.calls.contains(&"reapply_wsl"));
        let (error, line) = &machine.logs[0];
        assert!(*error && line.ends_with(why), "{line}");
    }

    #[test]
    fn a_run_not_allowed_touches_nothing_and_the_codes_are_their_own() {
        let mut machine = fake();
        machine.system = false;
        assert_eq!(wsl_upgrade(&mut machine), WslUpgradeExit::NotAllowed);
        assert_eq!(steps(&machine), Vec::<&str>::new());
        assert_eq!(machine.logs.len(), 1);
        let codes = WslUpgradeExit::ALL.map(WslUpgradeExit::as_code);
        assert_eq!(codes, [0, 60, 61, 62, 67]);
        for exit in WslUpgradeExit::ALL {
            assert_eq!(WslUpgradeExit::from_code(exit.as_code().into()), Some(exit));
        }
        // None is the deny task's (51, 52), in #717's 53 to 59 or the set-up's 63.
        let theirs = |code: u8| (51..=59).contains(&code) || code == 63;
        assert!(!codes.into_iter().any(theirs));
    }
}
