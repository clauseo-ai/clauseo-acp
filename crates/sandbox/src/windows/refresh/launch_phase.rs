//! The refresh's COM launch phase (ADR 0049, "COM launch restriction"): every run of
//! `\Clauseo\SandboxRefresh` re-applies the pool group's deny in `MachineLaunchRestriction`
//! (`machine_launch::apply`, idempotent: a run that finds it first writes nothing). It runs
//! after the admin copy's steps whatever they gave, but not when the run wasn't allowed (not
//! LOCAL SYSTEM, not the admin copy), the set-up lock was held (a set-up or removal holding it
//! does its own step), or the build carries the placeholder signing key, which refuses before
//! anything is read. It takes the lock itself, without waiting.
//!
//! Its outcome never changes the exit. Drift (a deny Clauseo wrote, gone) is an information
//! line saying it was put back; a degraded guard (a Group Policy value, a value that won't
//! parse, a failed write) is an error line, and its state is in the record for the app and
//! firm IT. A run that changed nothing and holds writes no line. No line names a path or a SID.

use super::{RefreshExit, Refresher};

/// After the admin copy's steps ended with `exit`: the deny re-applied as the module says,
/// unless the run wasn't allowed, the lock was held or the key is the placeholder; `exit`
/// either way.
pub(super) fn after(fs: &mut impl Refresher, exit: RefreshExit) -> RefreshExit {
    let skipped = [
        RefreshExit::NotAllowed,
        RefreshExit::LockHeld,
        RefreshExit::PlaceholderKey,
    ];
    if !skipped.contains(&exit) {
        run(fs);
    }
    exit
}

/// Re-applies the deny under the set-up lock, logging the outcome as the module says.
fn run(fs: &mut impl Refresher) {
    match fs.lock() {
        Ok(true) => {}
        Ok(false) => {
            fs.log(
                false,
                "the COM launch deny wasn't checked: the set-up lock is held",
            );
            return;
        }
        Err(why) => {
            fs.log(true, &format!("the COM launch deny wasn't checked: {why}"));
            return;
        }
    }
    let outcome = fs.reapply_launch_deny();
    fs.unlock();
    match outcome {
        Ok(None) => {}
        Ok(Some(applied)) => {
            if let Some(line) = applied.line() {
                fs.log(applied.guard.degraded() || applied.why.is_some(), &line);
            }
        }
        Err(why) => fs.log(true, &format!("the COM launch deny wasn't checked: {why}")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::fake::{Fake, good, key, new, old, run_fake};
    use super::super::{RefreshExit, SourceError};
    use crate::windows::machine_launch::{Change, Guard, LaunchApplied};
    use crate::windows::refresh::test_signer::TestKey;

    /// The fake machine, ready to refresh, and the key its exe is signed with.
    fn fake() -> (Fake, TestKey) {
        let key = key();
        (Fake::new(&old(), &new(), &key, &good()), key)
    }

    /// A guard `guard` whose run did `change`.
    fn applied(guard: Guard, change: Change, why: Option<&str>) -> LaunchApplied {
        LaunchApplied {
            guard,
            change,
            why: why.map(str::to_owned),
        }
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
        assert!(at("lock") < at("reapply_launch_deny"));
        assert!(at("reapply_launch_deny") < at("unlock"));
        assert_eq!(machine.calls.last(), Some(&"unlock"));
        // Nothing changed and the guard holds: no line of its own.
        assert_eq!(machine.logs.len(), 1, "{:?}", machine.logs);
        // A refused copy still re-applies, and keeps its own exit.
        let (mut machine, key) = fake();
        machine.source_exe = Err(SourceError::Missing);
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::SourceRefused);
        assert!(machine.calls.contains(&"reapply_launch_deny"));
    }

    #[test]
    fn drift_is_an_information_line_and_a_degraded_guard_an_error_line() {
        let (mut machine, key) = fake();
        machine.launch = Ok(Some(applied(Guard::Applied, Change::PutBack, None)));
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::Done);
        let (error, line) = machine.logs.last().unwrap();
        assert!(
            !*error && line.contains("missing (drift) and was put back"),
            "{line}"
        );
        let (mut machine, key) = fake();
        let why = "a Group Policy value overrides it";
        machine.launch = Ok(Some(applied(
            Guard::PolicyOverrides,
            Change::Unchanged,
            Some(why),
        )));
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::Done);
        let (error, line) = machine.logs.last().unwrap();
        assert!(*error, "{line}");
        assert_eq!(
            line,
            "MachineLaunchRestriction: degraded: group policy (a Group Policy value overrides it)"
        );
        // A put-back DCOM moved again: degraded, an error line.
        let (mut machine, key) = fake();
        let why = "dcom moved the value to MachineLaunchRestrictionOld";
        machine.launch = Ok(Some(applied(Guard::Moved, Change::Unchanged, Some(why))));
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::Done);
        let (error, line) = machine.logs.last().unwrap();
        assert!(*error, "{line}");
        assert!(line.contains("degraded: dcom moved the value"), "{line}");
        let (mut machine, key) = fake();
        machine.launch = Err("the record: access denied".to_owned());
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::Done);
        let (error, line) = machine.logs.last().unwrap();
        assert!(
            *error && line.starts_with("the COM launch deny wasn't checked"),
            "{line}"
        );
    }

    #[test]
    fn a_run_not_allowed_or_locked_out_leaves_the_deny_alone() {
        let (mut machine, key) = fake();
        machine.system = false;
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::NotAllowed);
        assert!(!machine.calls.contains(&"reapply_launch_deny"));
        let (mut machine, key) = fake();
        machine.fail = Some("lock");
        assert_eq!(run_fake(&mut machine, &key), RefreshExit::LockHeld);
        assert!(!machine.calls.contains(&"reapply_launch_deny"));
    }
}
