//! `--adopt` under `--deadline`: once the cancel flag is set it makes no further change, gives the
//! lock back and exits `DeadlineReached`; with no flag it runs as before.

use super::*;

#[test]
fn the_cancel_flag_stops_adopt_before_each_change() {
    let mut whole = Fake::new();
    assert_eq!(
        adopt(&mut whole, Path::new(INSTALL), &user()),
        SetupExit::Done
    );
    // The lock, three tasks and the values, then the set-up, which checks the flag itself.
    let changes = whole.changes.clone();
    assert_eq!(changes.len(), 6, "{changes:?}");
    for after in 0..5 {
        let mut fake = Fake::new();
        fake.cancel_after = Some(after);
        assert_eq!(
            adopt(&mut fake, Path::new(INSTALL), &user()),
            SetupExit::DeadlineReached,
            "{after}"
        );
        assert_eq!(fake.changes, changes[..after], "{after}");
        assert!(!fake.held, "the lock is given back");
        assert!(
            fake.reports.is_empty(),
            "a deadline is no failure to report"
        );
    }
    // Per machine: the lock, then the values.
    for after in 0..2 {
        let mut fake = Fake::new();
        fake.cancel_after = Some(after);
        let exit = adopt(&mut fake, Path::new(MACHINE), &Target::Machine);
        assert_eq!(exit, SetupExit::DeadlineReached, "{after}");
        assert_eq!(fake.changes.len(), after);
    }
}
