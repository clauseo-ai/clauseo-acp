//! `--wsl-deny`'s run lock and "run again" flag (`run_lock.rs`) on the fake machine.

use std::time::Duration;

use super::super::fake::{Fake, good, key, new, old};
use super::super::fake_lock::FakeLock;
use super::run_lock::{
    ADMIN_FOLDER_SDDL, AGAIN_VALUE, LOCK_FOLDERS, LOCK_NAME, LOCK_SDDL, system_only,
};
use super::{WSL_DENY_BUDGET, WslDenyExit, wsl_deny};
use crate::windows::admin_copy::fake::descriptor;

/// The fake machine, ready for the run.
fn fake() -> Fake {
    Fake::new(&old(), &new(), &key(), &good())
}

fn secs(secs: u64) -> Duration {
    Duration::from_secs(secs)
}

/// How many of `machine`'s calls were `name`.
fn count(machine: &Fake, name: &str) -> usize {
    machine.calls.iter().filter(|call| **call == name).count()
}

#[test]
fn a_start_while_a_run_holds_the_lock_sets_the_flag_and_exits_at_once() {
    let mut machine = fake();
    machine.run_lock = FakeLock {
        busy: 2,
        ..FakeLock::default()
    };
    assert_eq!(wsl_deny(&mut machine), WslDenyExit::Done);
    assert!(
        machine.run_lock.again,
        "the running one is asked for one more run"
    );
    assert!(!machine.run_lock.held);
    // Nothing read or waited for, no line: a forged flood costs processes that exit at once.
    let after_allowed: Vec<&str> = machine
        .calls
        .iter()
        .copied()
        .skip_while(|call| *call != "run_lock")
        .collect();
    assert_eq!(after_allowed, ["run_lock", "ask_again", "run_lock"]);
    assert_eq!(machine.watch.clock, Duration::ZERO);
    assert!(machine.logs.is_empty(), "{:?}", machine.logs);

    // The run let the lock go between the two tries: this start runs, clearing the flag.
    let mut machine = fake();
    machine.run_lock.busy = 1;
    assert_eq!(wsl_deny(&mut machine), WslDenyExit::Done);
    assert_eq!(count(&machine, "reapply_wsl"), 5);
    assert!(!machine.run_lock.again && !machine.run_lock.held);

    // The flag can't be set: one error line, and still no run beside the running one.
    let mut machine = fake();
    machine.run_lock.busy = 1;
    machine.run_lock.ask_fails = true;
    assert_eq!(wsl_deny(&mut machine), WslDenyExit::Done);
    assert_eq!(count(&machine, "reapply_wsl"), 0);
    assert_eq!(machine.logs.len(), 1);
    assert!(machine.logs[0].0, "{:?}", machine.logs);
}

#[test]
fn the_running_one_sees_the_flag_clears_it_and_runs_once_more() {
    let mut machine = fake();
    // Another start during the floor's passes.
    machine.run_lock.asks = vec![secs(10)];
    assert_eq!(wsl_deny(&mut machine), WslDenyExit::Done);
    // Read and cleared just before the end, at the floor's last pass; then one more full run.
    assert_eq!(machine.run_lock.taken, [secs(20)]);
    assert!(!machine.run_lock.again && !machine.run_lock.held);
    let floor = [0, 5, 10, 15, 20].map(secs);
    let again = [20, 25, 30, 35, 40].map(secs);
    assert_eq!(machine.watch.passes, [floor, again].concat());
    assert_eq!(count(&machine, "reapply_wsl"), 10);
    // One lock, let go once, then a last look for a start that came just before.
    assert_eq!(count(&machine, "run_lock"), 1);
    let tail = &machine.calls[machine.calls.len() - 2..];
    assert_eq!(tail, ["run_unlock", "asked_again"]);
    assert!(machine.logs.is_empty(), "{:?}", machine.logs);

    // A start before the run's first pass is covered by it: cleared, no second run.
    let mut machine = fake();
    machine.run_lock.again = true;
    assert_eq!(wsl_deny(&mut machine), WslDenyExit::Done);
    assert_eq!(count(&machine, "reapply_wsl"), 5);
    assert!(machine.run_lock.taken == [Duration::ZERO] && !machine.run_lock.again);
}

#[test]
fn a_flag_set_once_the_budget_is_spent_ends_the_run_for_the_next_trigger() {
    let mut machine = fake();
    // An install that outlasts the budget holds the run to it; a start comes meanwhile.
    machine.watch.in_progress = vec![(Duration::ZERO, secs(400))];
    machine.run_lock.asks = vec![secs(100)];
    assert_eq!(wsl_deny(&mut machine), WslDenyExit::Done);
    assert!(
        machine.watch.clock < WSL_DENY_BUDGET,
        "{:?}",
        machine.watch.clock
    );
    // Cleared, not run again: the process ends inside the task's limit.
    assert_eq!(machine.run_lock.taken.len(), 1);
    assert!(!machine.run_lock.again && !machine.run_lock.held);
    assert_eq!(count(&machine, "run_lock"), 1);
    assert!(!machine.calls.contains(&"asked_again"));
    let line = &machine.logs.last().unwrap().1;
    assert!(
        line.contains("asked to run again after its time limit"),
        "{line}"
    );
    assert_eq!(
        machine.watch.warnings.len(),
        1,
        "{:?}",
        machine.watch.warnings
    );
}

#[test]
fn a_lock_path_that_fails_its_checks_is_logged_and_the_run_goes_on_without_it() {
    for why in [
        "a folder of the run lock is a reparse point or a file",
        "the run lock isn't kept to SYSTEM",
    ] {
        let mut machine = fake();
        machine.run_lock.refused = Some(why.to_owned());
        assert_eq!(wsl_deny(&mut machine), WslDenyExit::Done);
        assert_eq!(count(&machine, "reapply_wsl"), 5, "the passes still run");
        assert_eq!(machine.logs.len(), 1, "{:?}", machine.logs);
        let (error, line) = &machine.logs[0];
        assert!(*error && line.contains("without its run lock") && line.ends_with(why));
        // No lock, so no flag: neither read nor set.
        for call in ["take_again", "ask_again", "asked_again", "run_unlock"] {
            assert!(!machine.calls.contains(&call), "{call}");
        }
    }
}

#[test]
fn the_lock_file_is_kept_to_system_and_its_folders_as_the_admin_only_log() {
    const FA: u32 = 0x001F_01FF;
    assert_eq!(LOCK_SDDL, "O:SYD:P(A;;FA;;;SY)");
    assert_eq!(LOCK_NAME, "wsl-deny.lock");
    assert_eq!(AGAIN_VALUE, "WslDenyAgain");
    assert_eq!(LOCK_FOLDERS.map(|(name, _)| name), ["Clauseo", "Sandbox"]);
    assert_eq!(LOCK_FOLDERS[1].1, ADMIN_FOLDER_SDDL);
    let system = [(0, 0, FA, "S-1-5-18")];
    assert!(system_only(&descriptor("S-1-5-18", Some(&system), true)));
    // Another owner, an inherited (unprotected) DACL, another entry, less than full control.
    assert!(!system_only(&descriptor(
        "S-1-5-32-544",
        Some(&system),
        true
    )));
    assert!(!system_only(&descriptor(
        "S-1-5-21-1-2-3-1001",
        Some(&system),
        true
    )));
    assert!(!system_only(&descriptor("S-1-5-18", Some(&system), false)));
    let users = [(0, 0, FA, "S-1-5-18"), (0, 0, 0x0012_0089, "S-1-5-32-545")];
    assert!(!system_only(&descriptor("S-1-5-18", Some(&users), true)));
    assert!(!system_only(&descriptor(
        "S-1-5-18",
        Some(&[(0, 0, 0x0012_0089, "S-1-5-18")]),
        true
    )));
    assert!(!system_only(&descriptor("S-1-5-18", None, true)));
}
