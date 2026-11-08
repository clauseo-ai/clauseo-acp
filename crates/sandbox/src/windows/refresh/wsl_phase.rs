//! The refresh's second phase (ADR 0049, "WSL"): every run of `\Clauseo\SandboxRefresh` (at the
//! owner's logon, on Windows Installer's events, on demand) re-applies WSL's deny for the pool
//! group (`wsl::apply`), since a WSL update or repair rewrites its launch permissions. It runs
//! after the admin copy's steps whatever they gave, but not when the run wasn't allowed (not
//! LOCAL SYSTEM, not the admin copy) or the set-up lock was held: a set-up or removal holding
//! it does its own WSL step. It takes the lock itself, without waiting.
//!
//! Its outcome never changes the exit: the launch check refuses a launch whose deny is missing
//! (`Blocked::WslDenyMissing`), so the deny's state is enforced there, and a failure here is one
//! Event Log error line, naming `AppID`s and Windows error codes, never a path or a SID. A run
//! that changed nothing writes no line.

use super::{RefreshExit, Refresher};

/// After the admin copy's steps ended with `exit`: WSL's deny re-applied as the module says,
/// unless the run wasn't allowed or the lock was held; `exit` either way.
pub(super) fn after(fs: &mut impl Refresher, exit: RefreshExit) -> RefreshExit {
    if !matches!(exit, RefreshExit::NotAllowed | RefreshExit::LockHeld) {
        run(fs);
    }
    exit
}

/// Re-applies WSL's deny under the set-up lock, logging the outcome as the module says.
fn run(fs: &mut impl Refresher) {
    match fs.lock() {
        Ok(true) => {}
        Ok(false) => {
            fs.log(false, "WSL's deny wasn't checked: the set-up lock is held");
            return;
        }
        Err(why) => {
            fs.log(true, &format!("WSL's deny wasn't checked: {why}"));
            return;
        }
    }
    let outcome = fs.reapply_wsl();
    fs.unlock();
    match outcome {
        Ok(None) => {}
        Ok(Some(line)) => fs.log(false, &line),
        Err(why) => fs.log(true, &format!("WSL's deny for the sandbox users: {why}")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake::{Fake, good, key, new, old, run_fake};
    use super::super::{RefreshExit, SourceError};
    use crate::windows::refresh::test_signer::TestKey;

    /// The fake machine, ready to refresh, and the key its exe is signed with.
    fn fake() -> (Fake, TestKey) {
        let key = key();
        (Fake::new(&old(), &new(), &key, &good()), key)
    }

    #[test]
    fn every_allowed_run_reapplies_under_the_lock_and_keeps_its_exit() {
        let (mut machine, key) = fake();
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::Done);
        let at = |name| {
            machine
                .calls
                .iter()
                .rposition(|call| *call == name)
                .unwrap()
        };
        assert!(at("lock") < at("reapply_wsl") && at("reapply_wsl") < at("unlock"));
        assert_eq!(machine.calls.last(), Some(&"unlock"));
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
        assert!(machine.calls.contains(&"reapply_wsl"));
    }

    #[test]
    fn a_run_not_allowed_or_locked_out_leaves_wsl_alone() {
        let (mut machine, key) = fake();
        machine.system = false;
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::NotAllowed);
        assert!(!machine.calls.contains(&"reapply_wsl"));
        let (mut machine, key) = fake();
        machine.fail = Some("lock");
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::LockHeld);
        assert!(!machine.calls.contains(&"reapply_wsl"));
    }
}
