//! The refresh's WSL phase (ADR 0049, "WSL"): every run of `\Clauseo\SandboxRefresh` (at the
//! owner's logon, on Windows Installer's events, on demand) re-applies WSL's deny for the pool
//! group (`wsl::apply`), since a WSL update or repair rewrites its launch permissions. It runs
//! twice in a run that is allowed (LOCAL SYSTEM, from the admin copy): first, before the admin
//! copy's steps, so the deny comes back without waiting for them; and last, whatever they gave,
//! so a run that began before an installer had finished re-applies after it too. The task
//! ignores a start while it runs, and its event trigger waits 15 s, so a burst of installer
//! events gives one run, which the second phase covers. Both are idempotent and cheap.
//!
//! Each waits up to [`WSL_LOCK_WAIT`] for the set-up lock, so a run that meets another
//! holder briefly (`--adopt`, another refresh's admin-copy steps) still re-applies; one still
//! held then is logged, and a set-up or removal holding it does its own WSL step.
//!
//! Its outcome never changes the exit: the launch check refuses a launch whose deny is missing
//! (`Blocked::WslDenyMissing`), so the deny's state is enforced there, and a failure here is one
//! Event Log error line, naming `AppID`s and Windows error codes, never a path or a SID. A run
//! that changed nothing writes no line.

use super::{Refresher, WSL_LOCK_WAIT};

/// Re-applies WSL's deny under the set-up lock, logging the outcome as the module says.
pub(super) fn run(fs: &mut impl Refresher) {
    match fs.lock_waiting() {
        Ok(true) => {}
        Ok(false) => {
            let secs = WSL_LOCK_WAIT.as_secs();
            fs.log(
                false,
                &format!("WSL's deny wasn't checked: the set-up lock was held for {secs} s"),
            );
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
}
